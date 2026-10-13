use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

const XML: &str = "assets/icons.gresource.xml";
const PRECOMPILED: &str = "assets/icons.gresource";

/// GResource dosyasının gerçekten gömülü bir kaynak olduğunu doğrular.
/// İki seriileştirme var: eski düz format (0xcaefa1ce) ve GVariant ("GVar").
/// GVariant dizeleri ofset tablosuyla sakladığı için ham ikili arama
/// (prefix aramak gibi) işe yaramaz; burada yalnız başlık imzası bakılır.
/// Asıl doğrulama çalışma anında `animecix --check-icons` ile yapılır.
fn looks_like_gresource(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GVar") || bytes.starts_with(&[0xce, 0xa1, 0xfa, 0xca])
}

fn main() {
    println!("cargo:rerun-if-changed=assets/hicolor");
    println!("cargo:rerun-if-changed={XML}");
    println!("cargo:rerun-if-changed={PRECOMPILED}");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dest_path = Path::new(&out_dir).join("icons.gresource");

    let has_compiler = Command::new("glib-compile-resources")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let mut problem: Option<String> = None;

    if Path::new(XML).exists() {
        if has_compiler {
            match Command::new("glib-compile-resources")
                .arg(format!("--target={}", dest_path.display()))
                .arg(XML)
                .status()
            {
                Ok(s) if s.success() => {}
                Ok(s) => problem = Some(format!("glib-compile-resources {s} ile döndü")),
                Err(e) => problem = Some(format!("glib-compile-resources çalıştırılamadı: {e}")),
            }
        } else {
            problem = Some(
                "glib-compile-resources yok (paket: libglib2.0-dev-bin / glib2-devel)".to_string(),
            );
        }
    } else {
        problem = Some(format!("{XML} bulunamadı"));
    }

    // Derlenemediyse önceden derlenmiş pakete düş; o da yoksa ya da bozuksa
    // sessizce geçme. Eksik ikonlar paketlenmiş binary'de kırık görünür.
    if problem.is_some() {
        let reason = problem.take().unwrap();
        match fs::read(PRECOMPILED) {
            Ok(bytes) if looks_like_gresource(&bytes) => {
                println!(
                    "cargo:warning=glib-compile-resources kullanılamadı ({reason}); \
                     önceden derlenmiş {PRECOMPILED} kopyalanıyor ({} bayt)",
                    bytes.len()
                );
                fs::copy(PRECOMPILED, &dest_path).expect("Önceden derlenmiş gresource kopyalanamadı");
            }
            Ok(bytes) => panic!(
                "{PRECOMPILED} ikon gresource'u değil ({} bayt, geçersiz imza). \
                 {reason}. `glib-compile-resources {XML}` ile yeniden üret.",
                bytes.len()
            ),
            Err(_) => panic!(
                "İkon gresource'u üretilemedi: {reason}, {PRECOMPILED} de yok. \
                 {XML} dosyasını ve assets/hicolor/scalable/actions içindeki SVG'leri \
                 paketleyeceğinden emin ol."
            ),
        }
    }

    // Son kontrol: binary'ye gömülecek baytlar geçerli bir kaynak mı?
    let bytes = fs::read(&dest_path).expect("Üretilen gresource okunamadı");
    assert!(
        looks_like_gresource(&bytes),
        "Üretilen {} geçerli bir gresource değil ({} bayt).",
        dest_path.display(),
        bytes.len()
    );
}
