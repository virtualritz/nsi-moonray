#!/usr/bin/env sh
#
# Offer to put a directory on the PATH, by appending a line to the
# start-up files the shells on this machine actually read.
#
# Usage: packaging/path.sh [--yes|--print] DIR
#
#   --yes     append without asking; for scripts and unattended installs
#   --print   print what would be appended and change nothing
#
# **Asks, and takes silence for no.** Editing somebody's shell
# configuration behind their back is the kind of helpfulness that gets
# an installer uninstalled, so without a terminal to ask at -- a
# pipeline, a CI job -- this prints the line and leaves the file alone.
#
# **Appends rather than prepends.** An install prefix holds binaries
# with ordinary names (MoonRay's `denoise`, `rdl2_print`), and putting
# them ahead of `/usr/bin` means a name collision changes which program
# the machine runs everywhere, not only here.

set -eu

MODE=ask
DIR=""

die() { echo "path: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --yes)     MODE=yes;   shift ;;
        --print)   MODE=print; shift ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) die "unknown argument $1" ;;
        *)  DIR="$1"; shift ;;
    esac
done

[ -n "$DIR" ] || die "usage: packaging/path.sh [--yes|--print] DIR"
[ -d "$DIR" ] || die "$DIR does not exist"
DIR="$(cd "$DIR" && pwd)"

case ":${PATH}:" in
    *":$DIR:"*) echo "path: $DIR is already on PATH"; exit 0 ;;
esac

# The login shell's own file first, because that is the one whose
# absence still means "write it" -- a machine where `~/.zshrc` has
# never existed still runs zsh. Everything after it is added only if it
# is already there, so this never invents a bash configuration for
# somebody who does not use bash.
home="${HOME:?HOME is not set}"
login_shell="$(basename "${SHELL:-sh}")"
files=""

add() {
    for seen in $files; do
        if [ "$seen" = "$1" ]; then return 0; fi
    done
    files="$files $1"
}

case "$login_shell" in
    zsh)  add "$home/.zshrc" ;;
    bash) add "$home/.bashrc"
          # macOS Terminal starts login shells, which read
          # `.bash_profile` and not `.bashrc`.
          if [ "$(uname -s)" = Darwin ]; then
              add "$home/.bash_profile"
          fi ;;
    fish) add "$home/.config/fish/config.fish" ;;
    *)    add "$home/.profile" ;;
esac

# `.profile` is deliberately not in this list: it is the fallback above
# for a shell nobody here recognises, and on the distributions that
# have one it already sources `.bashrc`, so naming both would put the
# directory on the PATH twice in one shell.
for other in "$home/.zshrc" "$home/.bashrc" "$home/.bash_profile" \
             "$home/.config/fish/config.fish"; do
    if [ -f "$other" ]; then add "$other"; fi
done

# fish is not a POSIX shell and `export PATH=` is a syntax error in it.
line_for() {
    case "$1" in
        *config.fish) echo "fish_add_path --append \"$DIR\"" ;;
        *)            echo "export PATH=\"\$PATH:$DIR\"" ;;
    esac
}

MARKER="# added by nsi-moonray"

pending=""
for file in $files; do
    if [ -f "$file" ] && grep -qF "$DIR" "$file"; then
        echo "path: $(basename "$file") already names it"
        continue
    fi
    pending="$pending $file"
done

[ -n "$pending" ] || exit 0

echo ""
echo "path: $DIR is not on your PATH. It holds:"
for name in moonray rdl2_print mnry; do
    if [ -x "$DIR/$name" ]; then echo "path:   $name"; fi
done
echo ""
for file in $pending; do
    echo "  $file:"
    echo "    $(line_for "$file")"
done
echo ""

# **Opened, not stat-ed, and in a subshell.** `[ -r /dev/tty ]` is true
# on a machine with no controlling terminal -- the device node is there
# and readable -- and the open then fails with ENXIO, which arrives as
# a shell error after the question has already been asked. Trying the
# open asks the same question the `read` below will. The subshell is
# not decoration: POSIX makes a redirection error on a special built-in
# fatal to the shell, so `{ : < /dev/tty; }` does not return false, it
# ends the script.
if [ "$MODE" = ask ]; then
    if ( : < /dev/tty ) 2>/dev/null; then
        printf "path: append that? [y/N] "
        if read -r answer < /dev/tty; then
            case "$answer" in
                y|Y|yes|Yes|YES) MODE=yes ;;
                *)               MODE=print ;;
            esac
        else
            MODE=print
        fi
    else
        MODE=print
    fi
fi

if [ "$MODE" = print ]; then
    echo "path: nothing changed. Copy the line above, or re-run with"
    echo "path:   packaging/path.sh --yes \"$DIR\""
    exit 0
fi

for file in $pending; do
    mkdir -p "$(dirname "$file")"
    # A blank line first, so this never joins onto an unterminated last
    # line somebody wrote by hand.
    printf '\n%s\n%s\n' "$MARKER" "$(line_for "$file")" >> "$file"
    echo "path: appended to $file"
done

echo "path: open a new shell, or source the file, to pick it up"
