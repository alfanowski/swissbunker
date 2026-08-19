#!/usr/bin/env sh
# SwissBunker bootstrap.
#
# Installs nothing on this machine. Everything — the daemon, the dashboard — is copied ONTO
# the disk you choose, so the computer you are sitting at is left exactly as it was. That is
# the whole promise of the product, and it starts here.
#
#   curl -fsSL https://swissbunker.sh | sh
#
# POSIX sh on purpose: this has to run on a stock macOS, a stock Linux, and whatever busybox
# a stranger's machine happens to have.
set -eu

RELEASE_BASE="${SWISSBUNKER_RELEASE:-https://github.com/alfanowski/swissbunker/releases/latest/download}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

detect_platform() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin) os=darwin ;;
        Linux)  os=linux ;;
        *) die "unsupported system: $os. Windows users: use install.ps1 instead." ;;
    esac
    case "$arch" in
        arm64|aarch64) arch=arm64 ;;
        x86_64|amd64)  arch=x64 ;;
        *) die "unsupported architecture: $arch" ;;
    esac
    printf '%s-%s' "$os" "$arch"
}

# List removable-looking volumes with their free space.
#
# Deliberately conservative: it lists what it can see and lets the operator choose, rather
# than guessing which disk is "the" bunker. Guessing wrong here means writing gigabytes to
# somebody's backup drive.
list_disks() {
    if [ -d /Volumes ]; then
        for v in /Volumes/*; do
            [ -d "$v" ] || continue
            free=$(df -h "$v" 2>/dev/null | awk 'NR==2 {print $4}')
            fstype=$(df "$v" 2>/dev/null | awk 'NR==2 {print $1}' | xargs -I{} sh -c 'diskutil info {} 2>/dev/null | awk -F: "/Type \(Bundle\)/ {gsub(/ /,\"\",\$2); print \$2}"' 2>/dev/null || echo "?")
            printf '%s\t%s\t%s\n' "$v" "${free:-?}" "${fstype:-?}"
        done
    else
        for v in /media/* /mnt/* /run/media/*/*; do
            [ -d "$v" ] || continue
            free=$(df -h "$v" 2>/dev/null | awk 'NR==2 {print $4}')
            fstype=$(df -T "$v" 2>/dev/null | awk 'NR==2 {print $2}')
            printf '%s\t%s\t%s\n' "$v" "${free:-?}" "${fstype:-?}"
        done
    fi
}

say ""
say "  SwissBunker — bootstrap"
say ""

platform=$(detect_platform)
say "  This machine: $platform"
say ""

disks=$(list_disks)
[ -n "$disks" ] || die "no mounted volumes found. Plug in your disk and run this again."

say "  Volumes available:"
say ""
i=0
printf '%s\n' "$disks" | while IFS="$(printf '\t')" read -r path free fs; do
    i=$((i + 1))
    printf '    %d) %-34s %8s free   %s\n' "$i" "$path" "$free" "$fs"
done
say ""

printf '  Which volume? [1] '
read -r choice </dev/tty || choice=1
[ -n "$choice" ] || choice=1
disk=$(printf '%s\n' "$disks" | sed -n "${choice}p" | cut -f1)
[ -n "$disk" ] && [ -d "$disk" ] || die "not a volume: ${choice}"

say ""
say "  Chosen: $disk"

# exFAT is the only filesystem writable on Windows, macOS and Linux without extra drivers.
# Anything else still works — on this operating system only — and the operator is told so
# rather than discovering it on a different computer, which is precisely the moment the whole
# product is supposed to work.
fs=$(printf '%s\n' "$disks" | sed -n "${choice}p" | cut -f3)
case "$fs" in
    *exfat*|*ExFAT*|*EXFAT*|*msdos*) : ;;
    *) say ""
       say "  WARNING: this volume is $fs, not exFAT."
       say "  It will work on this computer, but may be unreadable on Windows or Linux —"
       say "  which is the point of a portable bunker. Reformatting is your call and this"
       say "  script will not do it for you."
       printf '  Continue anyway? [y/N] '
       read -r ok </dev/tty || ok=n
       case "$ok" in y|Y|yes) : ;; *) die "stopped. Nothing was written." ;; esac
       ;;
esac

say ""
say "  Measuring write speed…"
# Five seconds well spent: it stops somebody committing six hours of indexing to a disk that
# cannot keep up, at the only moment when changing their mind is still cheap.
speed=$(dd if=/dev/zero of="$disk/.swissbunker-probe" bs=1m count=64 2>&1 | tail -1 | sed 's/.*, //' || echo "unknown")
rm -f "$disk/.swissbunker-probe"
say "    $speed"

say ""
say "  Installing onto the disk (nothing is written to this computer)…"
mkdir -p "$disk/bin/$platform" "$disk/app" "$disk/.state" "$disk/index"

if [ -n "${SWISSBUNKER_LOCAL:-}" ]; then
    # Development path: install from a checkout instead of a release.
    cp "$SWISSBUNKER_LOCAL/target/debug/swissbunkerd" "$disk/bin/$platform/" 2>/dev/null \
        || cp "$SWISSBUNKER_LOCAL/target/release/swissbunkerd" "$disk/bin/$platform/"
    cp "$SWISSBUNKER_LOCAL"/web/dist/* "$disk/app/" 2>/dev/null || true
else
    command -v curl >/dev/null || die "curl is required"
    curl -fsSL "$RELEASE_BASE/swissbunkerd-$platform" -o "$disk/bin/$platform/swissbunkerd" \
        || die "could not download the daemon for $platform"
    curl -fsSL "$RELEASE_BASE/app.tar.gz" | tar xz -C "$disk/app" \
        || die "could not download the dashboard"
fi
chmod +x "$disk/bin/$platform/swissbunkerd"

say "  Done."
say ""
say "  Start it with:"
say "    $disk/bin/$platform/swissbunkerd serve --disk $disk"
say ""
say "  Then open http://127.0.0.1:7777"
say ""
