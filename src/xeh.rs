//! Site istek imzası (X-E-H): AES-256-GCM ile sorgu imzalama.
//!
//! Angular istemcinin birebir kopyasıdır. Anahtar, format ve başlık
//! adı sitenin main bundle'ından çıkarıldı (`/browse` sayfasının
//! `getHeader` + `Jm` interceptor mantığı, 2026-09-13):
//! - düz metin: `"{version}" + sorgu-dizesi`
//! - anahtar: k1 + k2 ASCII birleşimi (32 bayt → AES-256)
//! - değer: `base64(sifreli).base64(iv)` (düz base64, 12 bayt rastgele IV)
//! - başlık adı: `X-E-H` (büyük/küçük harf duyarsız)

/// İmza anahtarı: `i4C7R2fXGocdYg` + `FLzCbDlsJ` + `jukf8G58b`.
const KEY: &[u8; 32] = b"i4C7R2fXGocdYgFLzCbDlsJjukf8G58b";

/// Sorgu dizesini imzala → `X-E-H` başlık değeri.
pub(crate) fn sign_query(query: &str) -> Result<String, String> {
    use base64::Engine as _;
    use ring::{aead, rand::SecureRandom};
    let rng = ring::rand::SystemRandom::new();
    let mut iv = [0u8; 12];
    rng.fill(&mut iv)
        .map_err(|_| "rastgele IV üretilemedi".to_string())?;
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, KEY)
            .map_err(|_| "imza anahtarı geçersiz".to_string())?,
    );
    let mut data = format!("{{version}}{query}").into_bytes();
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(iv),
        aead::Aad::empty(),
        &mut data,
    )
    .map_err(|_| "istek imzalanamadı".to_string())?;
    let eng = base64::engine::general_purpose::STANDARD;
    Ok(format!("{}.{}", eng.encode(&data), eng.encode(iv)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    #[test]
    fn sign_roundtrip_decrypts() {
        let query = "genre=drama,action&onlyStreamable=true&page=1&perPage=16";
        let v = sign_query(query).unwrap();
        let mut it = v.split('.');
        let ct_b64 = it.next().unwrap();
        let iv_b64 = it.next().unwrap();
        assert!(it.next().is_none(), "iki parçalı olmalı");
        let eng = base64::engine::general_purpose::STANDARD;
        let iv: [u8; 12] = eng
            .decode(iv_b64)
            .unwrap()
            .try_into()
            .expect("12 bayt IV");
        let key = ring::aead::LessSafeKey::new(
            ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, KEY).unwrap(),
        );
        let mut data = eng.decode(ct_b64).unwrap();
        let pt = key
            .open_in_place(
                ring::aead::Nonce::assume_unique_for_key(iv),
                ring::aead::Aad::empty(),
                &mut data,
            )
            .unwrap();
        assert_eq!(pt, format!("{{version}}{query}").as_bytes());
    }
}
