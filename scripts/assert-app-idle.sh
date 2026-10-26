#!/bin/zsh
set -euo pipefail

if (( $# != 1 )); then
  print -u2 "Usage: assert-app-idle.sh /path/to/Caskrun.app"
  exit 64
fi

APP="$1"
[[ -e "$APP" || -L "$APP" ]] || exit 0
if [[ -L "$APP" || ! -d "$APP" ]]; then
  print -u2 "Refusing to inspect an unrecognized app: $APP"
  exit 75
fi
if [[ "${APP:A}" != "${APP:a}" ]]; then
  print -u2 "Refusing to inspect an app through linked directories: $APP"
  exit 75
fi
APP="${APP:a}"

check_open_files() {
  local path="$1" scope="$2" report='' lsof_exit_code=0 pids
  if [[ -L "$path" || "${path:A}" != "${path:a}" ]]; then
    print -u2 "Refusing to inspect linked app components: $APP"
    exit 75
  fi
  [[ -e "$path" ]] || return 0
  if [[ "$scope" == tree ]]; then
    [[ -d "$path" ]] || {
      print -u2 "Refusing to inspect an unrecognized app component: $APP"
      exit 75
    }
  elif [[ ! -f "$path" ]]; then
    print -u2 "Refusing to inspect an unrecognized app component: $APP"
    exit 75
  fi

  if [[ "$scope" == tree ]]; then
    report=$(/usr/sbin/lsof -nP -Fpc +D "$path" 2>&1) || lsof_exit_code=$?
  else
    report=$(/usr/sbin/lsof -nP -Fpc -- "$path" 2>&1) || lsof_exit_code=$?
  fi
  pids=$(print -r -- "$report" | /usr/bin/awk '/^p[0-9]+$/ { print substr($0, 2) }')
  if [[ -n "$pids" ]]; then
    print -u2 "Refusing to replace an app in use: $APP (processes ${(j:,:)${(f)pids}})"
    exit 75
  fi
  # lsof exits 1 with no output when nothing has this file open. Any other
  # result without a PID is an inspection failure, so preserve the old app.
  if (( lsof_exit_code != 1 )) || [[ -n "$report" ]]; then
    print -u2 "Could not verify that the existing app is idle: $APP"
    exit 75
  fi
}

for executable in Caskrun caskrun-core caskrun-native-icon; do
  check_open_files "$APP/Contents/MacOS/$executable" file
done
check_open_files "$APP/Contents/Resources/wine-runtime" tree
