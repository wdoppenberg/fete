#!/usr/bin/env bash
#
# Fills the clip cache Terebi's video channel plays from.
#
#   tools/fetch-clips.sh [dest-dir]      # default: ./video
#
# Every source below is an off-air VHS capture of Japanese broadcast television
# held by the Internet Archive — Fuji TV, TBS and friends, 1985 to 1999. The
# call letters in the titles (JOCX, JOTX, JOKR, JODX) are the stations they were
# taped off, which is the level of provenance this material usually comes with.
#
# Everything here is domestic Japanese television: 風雲!たけし城, オレたちひょうきん族,
# 笑っていいとも!, 進ぬ!電波少年, quiz shows, off-air blocks, and anime — ゲゲゲの鬼太郎,
# おそ松くん, めぞん一刻, ドラゴンボール, クレヨンしんちゃん, デジモン, 超くせになりそう,
# and a Sanrio kids' tape. Two things were deliberately left out after looking
# at the frames:
#
#   * The general commercial compilations. Japanese ad breaks are a good fit for
#     this wall — short, saturated, graphic — but the compilations on the Archive
#     are a lottery, and the segments pulled came back with a Kodak ident in
#     English, a New York skyline and a European location shoot in them.
#   * 平成教育テレビ (1992). Good programme, but the segments contain a performer
#     in blackface, which early-90s Japanese variety used routinely. It is not
#     something to discover three metres tall behind a DJ.
#
# Nothing is downloaded whole. The Archive serves HTTP range requests, so ffmpeg
# seeks to the timecode over the network and pulls only the segment named here:
# a couple of megabytes per clip instead of the several hundred each of these
# tapes actually weighs. The whole cache is ~40 MB and takes a few minutes.
#
# The clips are cut to 320x240 — a little over the size one television is drawn
# on the wall — deinterlaced, silent, and at a constant 25 fps so the runtime
# decoder's `-re` pacing is uniform across the whole wall.
#
# Two segments per feed, roughly. Every set that is switched on is showing
# footage, so a wall of twenty-four feeds each looping a single seventy-five
# second clip would be visibly on a loop within a couple of minutes.
#
# This footage is broadcast television, uploaded as preservation. It is not
# public domain, and the Archive is not a licence. For a paid or ticketed room,
# point Terebi at your own folder instead: `--video ~/my-clips`. Anything ffmpeg
# reads works, and the same cut settings are worth applying.

set -euo pipefail

DEST="${1:-video}"
mkdir -p "$DEST"

# identifier | start (s) | length (s) | output name | file (optional)
#
# Start times are well inside each tape: the first minutes are usually leader,
# colour bars, or the tail of whatever was recorded over.
#
# The fifth field is only needed when an item holds more than one programme —
# the Takeshi's Castle upload is the whole 1986-1990 run in one item, so the
# episode has to be named or the metadata lookup just takes the first file.
CLIPS=$(cat <<'EOF'
tokoro-san-no-tadamo-no-dewanai-ore-tachi-hyoukin-zoku-jocx-tv-1989|900|75|hyoukin-1989-a
tokoro-san-no-tadamo-no-dewanai-ore-tachi-hyoukin-zoku-jocx-tv-1989|4200|75|hyoukin-1989-b
waratte-iitomo-jocx-tv-july-1991|600|75|iitomo-1991-a
waratte-iitomo-jocx-tv-july-1991|2100|75|iitomo-1991-b
1988_8_Kuizu_Chikyuu_Maru_Kajiri_November_1988|540|75|quiz-marukajiri-1988
1991-27|300|75|takada-variety-1991
vhs11_202509|1800|75|denpa-shonen-1998-a
vhs11_202509|5400|75|denpa-shonen-1998-b
3-hours-of-japanese-tv-1988|1500|75|offair-1988-a
3-hours-of-japanese-tv-1988|7200|75|offair-1988-b
003-1986-05-16|300|75|takeshi-1986-a|003_1986-05-16.mp4
003-1986-05-16|1500|75|takeshi-1986-b|003_1986-05-16.mp4
003-1986-05-16|2350|75|takeshi-1986-c|003_1986-05-16.mp4
003-1986-05-16|900|75|takeshi-monster-1987|062_1987-10-02_Monster Special.mp4
003-1986-05-16|1400|75|takeshi-1000-1990|133_1990-10-19_1000 Contestant Attack Special.mp4
ge-ge-ge-no-kitaro-episode-85-kappa-ichizoku-to-takuro-bi-jocx-tv-06-04-87|300|75|kitaro-1987-a
ge-ge-ge-no-kitaro-episode-85-kappa-ichizoku-to-takuro-bi-jocx-tv-06-04-87|1000|75|kitaro-1987-b
osomatsu-kun-ep-10-jocx-tv-04-27-88|200|75|osomatsu-1988|Osomatsu-kun  EP 10 - 地獄の死神セールスマン！！[JOCX-TV, 04-27-88].mp4
maison-ikkoku-episode-96-jocx-tv-03-03-88|400|75|maison-ikkoku-1988|maison ikkoku 96.mp4
DragonBall-VHS-Captures|600|75|dragonball-a|#148 やった! 地球上最強の男.mp4
DragonBall-VHS-Captures|1150|75|dragonball-b|#148 やった! 地球上最強の男.mp4
shinchan-1997-newyear-special|900|75|shinchan-1997|クレヨンしんちゃん お正月スペシャル.ia.mp4
digimon-adventure-vol-1-13-1999-2000-japanese-vhs|400|75|digimon-1999|Digimon Adventure - Vol. 01 (1999 Japanese VHS).mp4
cho-kuse-ni-nariso-episodes|300|75|chokuse-1994|s01e23.ja.mp4
anime-and-norimono|200|75|sanrio-1993|1993 SAVV-608 サンリオおもしろ図鑑 ハローキティのあつまれ!でんしゃ.mp4
tokoro-san-no-tadamo-no-dewanai-ore-tachi-hyoukin-zoku-jocx-tv-1989|2400|75|hyoukin-1989-c
tokoro-san-no-tadamo-no-dewanai-ore-tachi-hyoukin-zoku-jocx-tv-1989|5600|75|hyoukin-1989-d
waratte-iitomo-jocx-tv-july-1991|1300|75|iitomo-1991-c
waratte-iitomo-jocx-tv-july-1991|2900|75|iitomo-1991-d
1988_8_Kuizu_Chikyuu_Maru_Kajiri_November_1988|1600|75|quiz-marukajiri-b
1988_8_Kuizu_Chikyuu_Maru_Kajiri_November_1988|2200|75|quiz-marukajiri-c
1991-27|900|75|takada-variety-b
1991-27|1300|75|takada-variety-c
vhs11_202509|3000|75|denpa-shonen-1998-c
vhs11_202509|7000|75|denpa-shonen-1998-d
3-hours-of-japanese-tv-1988|3600|75|offair-1988-c
3-hours-of-japanese-tv-1988|9500|75|offair-1988-d
003-1986-05-16|700|75|takeshi-1986-d|003_1986-05-16.mp4
003-1986-05-16|1900|75|takeshi-1986-e|003_1986-05-16.mp4
003-1986-05-16|1200|75|takeshi-1986-05-30|005_1986-05-30.mp4
003-1986-05-16|1500|75|takeshi-regional-1986|021_1986-10-31_Regional Special.mp4
003-1986-05-16|800|75|takeshi-newyear-1987|028_1987-01-02_New Year Special_DVD.mp4
003-1986-05-16|2500|75|takeshi-monster-b|062_1987-10-02_Monster Special.mp4
003-1986-05-16|2600|75|takeshi-1000-b|133_1990-10-19_1000 Contestant Attack Special.mp4
ge-ge-ge-no-kitaro-episode-85-kappa-ichizoku-to-takuro-bi-jocx-tv-06-04-87|1500|75|kitaro-1987-c
osomatsu-kun-ep-10-jocx-tv-04-27-88|700|75|osomatsu-1988-b|Osomatsu-kun  EP 10 - 地獄の死神セールスマン！！[JOCX-TV, 04-27-88].mp4
osomatsu-kun-ep-10-jocx-tv-04-27-88|1100|75|osomatsu-1988-c|Osomatsu-kun EP 12 - 売れっ子小説家 イヤミ大先生 [JOCX-TV, 05-07-88].mp4
maison-ikkoku-episode-96-jocx-tv-03-03-88|900|75|maison-ikkoku-b|maison ikkoku 96.mp4
maison-ikkoku-episode-96-jocx-tv-03-03-88|1400|75|maison-ikkoku-c|maison ikkoku 96.mp4
DragonBall-VHS-Captures|250|75|dragonball-c|#148 やった! 地球上最強の男.mp4
shinchan-1997-newyear-special|1800|75|shinchan-1997-b|クレヨンしんちゃん お正月スペシャル.ia.mp4
shinchan-1997-newyear-special|2800|75|shinchan-1997-c|クレヨンしんちゃん お正月スペシャル.ia.mp4
digimon-adventure-vol-1-13-1999-2000-japanese-vhs|1500|75|digimon-1999-b|Digimon Adventure - Vol. 01 (1999 Japanese VHS).mp4
digimon-adventure-vol-1-13-1999-2000-japanese-vhs|3000|75|digimon-1999-c|Digimon Adventure - Vol. 02 (1999 Japanese VHS).mp4
cho-kuse-ni-nariso-episodes|700|75|chokuse-1994-b|s01e23.ja.mp4
cho-kuse-ni-nariso-episodes|1100|75|chokuse-1994-c|s01e24.ja.mp4
anime-and-norimono|700|75|sanrio-1993-b|1993 SAVV-608 サンリオおもしろ図鑑 ハローキティのあつまれ!でんしゃ.mp4
anime-and-norimono|400|75|sanrio-1993-c|1993 SAVV-609 サンリオおもしろ図鑑 ハローキティのあつまれ!いろんなくるま.mp4
EOF
)

# The Archive keeps a normalised h.264 derivative of most uploads. Resolving it
# from the metadata API rather than hardcoding filenames means a re-derived item
# does not silently break the fetch.
# Whether a cached clip is actually playable and roughly the right length.
#
# Worth the ffprobe: a dropped connection mid-transfer leaves ffmpeg exiting
# zero with a truncated file that has no moov atom, and a plain -s test calls
# that a cache hit forever. The show plays whatever is in this directory, so a
# file that is here has to be a file that works.
valid() {
  local want="$2"
  local got
  got=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$1" 2>/dev/null) || return 1
  [ -n "$got" ] || return 1
  awk -v g="$got" -v w="$want" 'BEGIN { exit !(g > w * 0.6) }'
}

resolve() {
  curl -sf "https://archive.org/metadata/$1" | python3 -c '
import json, sys, urllib.parse
d = json.load(sys.stdin)
files = d.get("files", [])
pick = [f for f in files if f.get("format") == "h.264"] \
    or [f for f in files if f.get("name", "").lower().endswith((".mp4", ".mkv", ".mpg", ".avi"))]
if not pick:
    sys.exit(1)
print(urllib.parse.quote(pick[0]["name"]))
'
}

total=$(printf '%s\n' "$CLIPS" | wc -l | tr -d ' ')
n=0
while IFS='|' read -r id start len name want; do
  [ -n "$id" ] || continue
  n=$((n + 1))
  out="$DEST/$name.mp4"

  if [ -s "$out" ]; then
    if valid "$out" "$len"; then
      printf '[%2d/%2d] %-24s cached\n' "$n" "$total" "$name"
      continue
    fi
    printf '[%2d/%2d] %-24s re-fetching (cached copy is truncated)\n' "$n" "$total" "$name"
    rm -f "$out"
  fi

  printf '[%2d/%2d] %-24s ' "$n" "$total" "$name"
  if [ -n "$want" ]; then
    file=$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$want")
  elif ! file=$(resolve "$id"); then
    printf 'SKIP (no video file on %s)\n' "$id"
    continue
  fi

  # -ss ahead of -i is an input seek: over HTTP that is a range request, so
  # only the bytes around the timecode are ever fetched.
  if ffmpeg -nostdin -hide_banner -loglevel error \
      -ss "$start" -i "https://archive.org/download/$id/$file" -t "$len" -an \
      -vf "yadif=0:-1:0,scale=320:240:force_original_aspect_ratio=increase,crop=320:240,fps=25" \
      -c:v libx264 -preset veryfast -crf 24 -pix_fmt yuv420p -movflags +faststart \
      -y "$out" 2>/dev/null && valid "$out" "$len"; then
    printf 'ok (%s)\n' "$(du -h "$out" | cut -f1 | tr -d ' ')"
  else
    # Truncated as often as refused: the Archive drops long range requests, and
    # ffmpeg exits zero having written a file with no index in it.
    printf 'FAILED (retry — the Archive drops these intermittently)\n'
    rm -f "$out"
  fi
done <<< "$CLIPS"

echo
echo "$(ls -1 "$DEST"/*.mp4 2>/dev/null | wc -l | tr -d ' ') clips in $DEST/ ($(du -sh "$DEST" | cut -f1 | tr -d ' '))"
