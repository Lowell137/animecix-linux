#!/usr/bin/env bash
# AnimeciX Otomatik Kurulum ve Başlatma Betiği
# Desteklenen dağıtımlar: Fedora, Ubuntu, Debian, Arch Linux, openSUSE ve türevleri.
set -euo pipefail

REPO="Lowell137/animecix-linux"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
TARGET_APPIMAGE="$INSTALL_DIR/AnimeciX-x86_64.AppImage"
SYMLINK_BIN="$INSTALL_DIR/animecix"

echo "========================================="
echo "   AnimeciX Otomatik Kurulum ve Başlatıcı"
echo "========================================="

# 1. Dağıtım tespiti
DISTRO=""
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO="${ID:-}"
    ID_LIKE="${ID_LIKE:-}"
else
    echo "Hata: /etc/os-release bulunamadı." >&2
    exit 1
fi

echo "==> Dağıtım tespit edildi: $NAME ($DISTRO)"

# 2. Bağımlılıkların tespiti ve yüklenmesi
install_deps() {
    local cmd_prefix=""
    if [ "$(id -u)" -ne 0 ]; then
        if command -v sudo >/dev/null 2>&1; then
            cmd_prefix="sudo"
        elif command -v doas >/dev/null 2>&1; then
            cmd_prefix="doas"
        else
            echo "Hata: Root yetkisi gerekiyor ancak sudo/doas bulunamadı." >&2
            exit 1
        fi
    fi

    case "$DISTRO" in
        fedora|rhel|centos|rocky|alma)
            echo "==> Fedora/RHEL bağımlılıkları kuruluyor (fuse-libs, mpv-libs, mpv, gtk4, libadwaita, curl, jq)..."
            $cmd_prefix dnf install -y fuse-libs mpv-libs mpv gtk4 libadwaita curl jq
            ;;
        arch|manjaro|endeavouros|cachyos|artix)
            echo "==> Arch bağımlılıkları kuruluyor (fuse2, mpv, gtk4, libadwaita, curl, jq)..."
            $cmd_prefix pacman -S --needed --noconfirm fuse2 mpv gtk4 libadwaita curl jq
            ;;
        ubuntu|debian|pop|linuxmint|elementary|zorin)
            echo "==> Debian/Ubuntu paket listeleri güncelleniyor..."
            $cmd_prefix apt-get update -y
            
            # Ubuntu 24.04+ ve Debian 13+ libfuse2t64 kullanır, eskiler libfuse2
            local fuse_pkg="libfuse2"
            if apt-cache show libfuse2t64 >/dev/null 2>&1; then
                fuse_pkg="libfuse2t64"
            fi
            
            echo "==> Bağımlılıklar kuruluyor ($fuse_pkg, libmpv2/libmpv1, mpv, libgtk-4-1, libadwaita-1-0, curl, jq)..."
            $cmd_prefix apt-get install -y "$fuse_pkg" libmpv2 mpv libgtk-4-1 libadwaita-1-0 curl jq || \
            $cmd_prefix apt-get install -y "$fuse_pkg" libmpv1 mpv libgtk-4-1 libadwaita-1-0 curl jq
            ;;
        opensuse*|suse)
            echo "==> openSUSE bağımlılıkları kuruluyor (libfuse2, mpv, gtk4, libadwaita-1-0, curl, jq)..."
            $cmd_prefix zypper --non-interactive install libfuse2 mpv gtk4 libadwaita-1-0 curl jq
            ;;
        *)
            if [[ "$ID_LIKE" == *"arch"* ]]; then
                $cmd_prefix pacman -S --needed --noconfirm fuse2 mpv gtk4 libadwaita curl jq
            elif [[ "$ID_LIKE" == *"debian"* ]] || [[ "$ID_LIKE" == *"ubuntu"* ]]; then
                $cmd_prefix apt-get update -y
                $cmd_prefix apt-get install -y libfuse2 mpv curl jq || $cmd_prefix apt-get install -y libfuse2t64 mpv curl jq
            elif [[ "$ID_LIKE" == *"fedora"* ]] || [[ "$ID_LIKE" == *"rhel"* ]]; then
                $cmd_prefix dnf install -y fuse-libs mpv-libs mpv gtk4 libadwaita curl jq
            else
                echo "Uyarı: Bilinmeyen dağıtım ($DISTRO). Lütfen FUSE 2 (fuse-libs/libfuse2/fuse2) ve mpv paketlerini kendiniz kurun."
            fi
            ;;
    esac
}

# Bağımlılıkları kontrol et ve gerekirse kur
NEEDS_INSTALL=0

# libmpv paylaşılan kütüphane kontrolü (libmpv.so.2 / libmpv.so)
if ! ldconfig -p 2>/dev/null | grep -q "libmpv\.so"; then
    NEEDS_INSTALL=1
fi

# FUSE2 kontrolü (libfuse.so.2)
if ! ldconfig -p 2>/dev/null | grep -q "libfuse\.so\.2"; then
    NEEDS_INSTALL=1
fi

if [ "$NEEDS_INSTALL" -eq 1 ]; then
    echo "==> Eksik sistem bağımlılıkları tespit edildi, yükleniyor..."
    install_deps
else
    echo "==> Tüm temel bağımlılıklar (FUSE2, libmpv) zaten kurulu."
fi

# 3. Dizinleri oluştur
mkdir -p "$INSTALL_DIR"
mkdir -p "$DESKTOP_DIR"
mkdir -p "$ICON_DIR"

# 4. En güncel AppImage sürümünü tespit et ve indir
echo "==> GitHub'dan en güncel sürüm sorgulanıyor..."
LATEST_URL=""
if command -v curl >/dev/null 2>&1; then
    LATEST_URL=$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
        | grep -o 'https://github.com/[^"]*AnimeciX-x86_64\.AppImage' | head -n1 || true)
fi

if [ -z "$LATEST_URL" ]; then
    LATEST_URL="https://github.com/$REPO/releases/latest/download/AnimeciX-x86_64.AppImage"
fi

echo "==> AnimeciX AppImage indiriliyor ($LATEST_URL)..."
curl -sSL -o "$TARGET_APPIMAGE" "$LATEST_URL"
chmod +x "$TARGET_APPIMAGE"
ln -sf "$TARGET_APPIMAGE" "$SYMLINK_BIN"

# 5. İkonu indir / ayarla
ICON_PATH="$ICON_DIR/tr.com.animecix.png"
if [ ! -f "$ICON_PATH" ]; then
    echo "==> Uygulama ikonu indiriliyor..."
    curl -sSL -o "$ICON_PATH" "https://raw.githubusercontent.com/$REPO/main/assets/hicolor/256x256/apps/tr.com.animecix.png" || true
fi

# 6. .desktop başlatıcı dosyası oluştur (Menü ve arama için)
DESKTOP_FILE="$DESKTOP_DIR/tr.com.animecix.desktop"
cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Name=AnimeciX
Comment=Türkçe Anime İzleme Uygulaması
Exec=$TARGET_APPIMAGE %U
Icon=tr.com.animecix
Terminal=false
Type=Application
Categories=AudioVideo;Video;Network;
StartupWMClass=tr.com.animecix
MimeType=x-scheme-handler/animecix;
EOF
chmod +x "$DESKTOP_FILE"

# Masaüstü veritabanını güncelle
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
fi

echo ""
echo "========================================="
echo "   Kurulum Başarıyla Tamamlandı!"
echo "   Konum: $TARGET_APPIMAGE"
echo "   Terminalden: animecix veya doğrudan menüden başlatabilirsiniz."
echo "========================================="
echo ""

# 7. Uygulamayı başlat
echo "==> AnimeciX başlatılıyor..."
if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
    nohup "$TARGET_APPIMAGE" "$@" >/dev/null 2>&1 &
    disown || true
    echo "==> Arka planda başlatıldı."
else
    echo "Uyarı: Grafik oturumu (DISPLAY/WAYLAND_DISPLAY) bulunamadı. Terminalden başlatmak için: $TARGET_APPIMAGE"
fi
