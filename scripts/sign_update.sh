#!/usr/bin/env bash
# AppImage sürüm imzalacı: ECDSA P-256 (SHA-256) → <dosya>.sig (base64 DER).
#
# Kullanım:  scripts/sign_update.sh AnimeciX-x86_64.AppImage
# Yayın:     gh release upload v<X.Y.Z> AnimeciX-x86_64.AppImage.sig
#
# Özel anahtar: ~/.config/animecix-signing/update-sign-key.pem
# (ANIMECIX_SIGN_KEY ile değiştirilebilir). Public anahtar src/update.rs
# içindeki UPDATE_PUBKEY_HEX sabitindedir; anahtar değişirse güncellenmeli.
set -euo pipefail

KEY="${ANIMECIX_SIGN_KEY:-$HOME/.config/animecix-signing/update-sign-key.pem}"
FILE="${1:?Kullanım: sign_update.sh <dosya-yolu>}"

[ -f "$FILE" ] || { echo "Dosya bulunamadı: $FILE" >&2; exit 1; }
if [ ! -f "$KEY" ]; then
  echo "İmza anahtarı yok: $KEY" >&2
  echo "Üretmek için: mkdir -p ~/.config/animecix-signing && openssl ecparam -name prime256v1 -genkey -noout -out ~/.config/animecix-signing/update-sign-key.pem" >&2
  exit 1
fi

openssl dgst -sha256 -sign "$KEY" -out "$FILE.sig.der" "$FILE"
base64 -w0 "$FILE.sig.der" > "$FILE.sig"
rm "$FILE.sig.der"
echo "==> İmzalandı: $FILE.sig ($(sha256sum "$FILE" | cut -c1-16)...)"
