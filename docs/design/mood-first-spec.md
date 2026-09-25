# AnimeciX v2 — "Mod Önce" Tasarım Spesifikasyonu

> Kaynaklar: (1) animecix.tv'nin 2026-09-21 tarihli gerçek IA'sı (playwright ile
> yerinde incelendi), (2) kullanıcının mood-selector / sosyal feed / algoritma
> geri bildirimi prompt'u. Netflix klonu DEĞİL — sitenin kendi veri gerçekliği
> üzerine kurulu, masaüstüne özgü bir keşif dili.

## 0. Siteden Çıkarılan Gerçekler (tasarımın oturduğu zemin)

- **Home**: dönen spotlight ("Sezonun İncileri" + Haberler) → Son Eklenen Bölümler →
  Son Çıkan Animeler → En Yüksek Puanlı → Çok Oy Alan Movieler → Gelecek Animeler.
  Yani site zaten "ray" mantığında; ama ray'lar **istatistik** temelli, **kişi** değil.
- **Browse**: tür / seri tipi / ülke / dil / yaş sınırı / "sadece izlenebilenler" + puan sort.
  Filtre kümesi zengin ama düz bir form — keşif motoru değil, katalog araması.
- **Detay**: tam-genişlik trailer hero + poster + meta (süre, bölüm sayısı, tarih) +
  türler + tag'ler (MAGIC, FRIENDSHIP, IMMORTALITY...) + Sezonlar/Bölümler/Videolar/
  Derecelendirmeler/Oyuncular sekmeleri. Tag'ler mood eşlemesi için hazır malzeme.
- **Oynatma sayfası**: player + yükleyen hayran grubu (credits: çeviren/redaktör/encoder) +
  **"Diğer Çeviriler"** (aynı bölümün alternatif fansub'ları!) + sağda ilişkililer +
  "Oto. Sonraki Bölüm" ve **"Sahne Işıkları"** toggle'ları. Bu üçü sitenin ruhu:
  emek görünürlüğü, seçenek, atmosfer. v2 bunları yüzeyselleştirmez, büyütür.
- **Takvim**: gün kolonları (Pazartesi...) — haftalık ritim zaten var.
- **Sosyal gerçeklik**: kullanıcı profilleri, derecelendirmeler/eleştiriler, listeler,
  Discord. Arkadaş sistemi site API'sinde sınırlı → sosyal feed **takip + yerel** melez olur.

## 1. Konsept: "Rafa kalk, modunu söyle"

Uygulama açılınca kullanıcı bir katalogla değil, tek soruyla karşılaşır:

> **"Şu an ne hissediyorsun?"**

**Mood çemberi** — 7 kapı, her biri sitenin tag/tür/puan verisinden derlenen bir kürasyon:

| Kapı | Eşleşen veri (site gerçekliği) | Süre damarı |
|---|---|---|
| 🌧 Melankoli | Drama, Iyashiyoku değil; yavaş tempolu, uzun soluk | 24+ dk |
| ☕ Huzur / Iyashi | Komedi+Drama, Slice of Life tag'leri | 24 dk |
| 🔥 Adrenalin | Action & Adventure, yüksek "çok oy alan" | film veya hızlı seri |
| 🧩 Düşündürü | Sci-Fi & Fantasy, Mystery, R/PG-13, yüksek puan | 12-24 dk |
| 😂 Kafamı boşatayım | Comedy, Ecchi, düşük eşiklik | 12-24 |
| 🎭 Epik yolculuk | Adventure+Fantasy, çok sezonlu, marathon'a uygun | uzun |
| 🌙 Gece yarısı | Korku/Thriller/Psychological, kısa sezon | 22:00 sonrası öne çıkar |

Mood bir **oturum** açar: ekranın teması, ray'ların sırası, hatta takvim vurgusu o moda
ayarlanır. Ama hiçbir zaman bir duvar değildir — üst çubukta tek dokunuşla modlar arası geçiş.

## 2. Information Architecture — 3 Seviyeli Ağaç

```
1. KEŞİF (Home)
├── 1.1 Mood Çemberi
│   ├── 1.1.1 Mood oturumu: "Senin için" ray'ı + 3 alt-ray (yeni/tamamlanan/koleksiyon)
│   ├── 1.1.2 Mood playlist'i (otomatik derlenen, kalıcılaştırılabilir "Vibe listeleri")
│   └── 1.1.3 Mod geçişi / "Karıştır" (iki mood'un kesişimi → sürpriz ray)
├── 1.2 Katalog
│   ├── 1.2.1 Browse (site filtreleri: tür/tip/ülke/dil/yaş + "sadece izlenebilen")
│   ├── 1.2.2 Arama (anlık sonuç + son aramalar)
│   └── 1.2.3 Listeler (En İyi Movieler / Gelecek Animeler / site listeleri)
└── 1.3 Haftalık ritim
    ├── 1.3.1 Takvim (gün kolonları → "bu gün" vurgulu, favorilere göre sıralı)
    └── 1.3.2 Yeni bölüm akışı (takip edilen yapımlardan gelenler)

2. YAPIM (Detay)
├── 2.1 Karar yüzeyi
│   ├── 2.1.1 Hero (trailer + meta + tag'ler + mood rozetleri: "Huzur · Yavaş yanık")
│   └── 2.1.2 Nereden izleyeyim: Sezonlar → Bölümler → **Çeviri (fansub) seçimi**
├── 2.2 Koleksiyon
│   ├── 2.2.1 Listem / durum (izleniyor, planlı, bitirdi) + yeni bölüm bildirimi
│   └── 2.2.2 Maraton kuyruğuna ekle
└── 2.3 Topluluk
    ├── 2.3.1 Derecelendirmeler & eleştiriler (site içeriği, uygulama içi okuma)
    └── 2.3.2 Yükleyen grubun diğer işleri (fansub-discoverability)

3. OYNATMA
├── 3.1 Player
│   ├── 3.1.1 Kontroller + Sahne Işıkları + kaynak/kalite/hız
│   └── 3.1.2 OP/ED atlama (AniSkip) + bölüm içi ilerleme
├── 3.2 Bölüm sonu döngüsü
│   ├── 3.2.1 Sonraki bölüm geri sayımı (oto-geç toggle'la)
│   └── 3.2.2 **Tek dokunuş geri bildirim**: 👍 bu modu sürdür / 🔄 modu değiştir / ⏭ geç
└── 3.3 Oturum zekâsı
    ├── 3.3.1 Canlı ağırlıklandırma (geri bildirimler oturum ray'larını anında yeniden dizer)
    └── 3.3.2 Oturum özeti: "Bu gece 2 saat Huzur modundaydın" → kalıcı profilin tohumu

4. SOSYAL (yan panel, her yerden erişilir)
├── 4.1 "Şimdi ne izliyorlar" — takip edilen kullanıcıların yerel oturumları
│   (uygulama paylaşımı opt-in; Discord'da "şu an" durumu köprüsü)
├── 4.2 Fansub nabzı — takip edilen çeviri gruplarının yeni yüklemeleri
└── 4.3 Haftalık sandık: arkadaşların bitirdiği + senin listende kesişenler
```

## 3. Geri Bildirim Döngüsü (algoritma — istemci tarafında, dürüst)

Sunucu "algoritma" yok; olmasına da gerek yok. Model:

- Her yapımın vektörü: tür + tag + mood-eşlemi + puan + sezon uzunluğu + tempo.
- Kullanıcının **oturum ağırlıkları**: mood çemberinden başlangıç (0.7), her geri
  bildirimle ±0.15; 3 bölüm üst üste 👍 → mood "kilidi" önerisi.
- Kalıcı profil = son 30 günün oturum ağırlıklarının üstel ortalaması; "Neden bunu
  görüyorum?" linki her ray'da — tıklanınca hangi özellik tetiklediğini gösterir
  (açıkça: "Gece yarısı modunda izlediğin 4 psikolojik gerilim").
- Kural: **ray asla aynı mood'un 3'ünden fazlasını arka arkaya vermez** — keşif
  süzülür ama tek yörüngeye çökmez.

## 4. Görsel Dil (kopya değil, kendi karakteri)

- **Zemin**: `#0C0D10` mürekkep; yüzeyler `#15171C`/`#1C1F26`. Klon farkı: Netflix
  nötr gri kullanır; AnimeciX **modun rengine** hafif boyanır (melankoli → lacivert
  sis, adrenalin → kömür+kızıl vinyet) — 8-12% opaklıkta, fark edilmez ama hissedilir.
- **Aksan**: mood başına renk ataması (tek marka kırmızısı yerine): Huzur `#7FB77E`,
  Melankoli `#6C7FB8`, Adrenalin `#E2593B`, Düşündürü `#B98CD4`, Gece `#4A5568`...
  Mood çemberi arayüzün renk paletini sürer → arayüz "konuşur", Netflix gibi susmaz.
- **Tipografi**: başlık **Bricolage Grotesque** (karakterli, Türkçe uyumlu), gövde Inter.
- **Işıltı değil kağıt**: posterler hafif film-grain overlay; kart radius 10px;
  hover'da Netflix'in agresif scale'i yerine **yumuşak yükselme + gölge** (2-4px).
- **Fansub imzası**: her kartta küçük çeviri-grubu rozeti — sitenin emek kültürünü
  arayüz sahiplenir. Bu, hiçbir streaming klonunda olmayan özgünlük.

## 5. Erişilebilirlik

- Mood çemberi: daireyi klavyeyle gezmek zor — **çember + eşdeğer liste** (radio group,
  `aria-label="Mod seçimi"`), dokunmatik hedef ≥ 48px, kontrast AA (mood renkleri
  koyu zeminde ≥ 4.5:1; geçmeyenler metinde değil sadece vurguda kullanılır).
- "Neden bunu görüyorum?" ve geri bildirim butonları ekran-okuyucu etiketli;
  bölüm-sonu geri sayım overlay'i `aria-live="polite"`, İptal odaklı başlar.
- `prefers-reduced-motion`: mood renk geçişleri anında, kart animasyonları kalkar.
- Sahne Işıkları parlaklık düşürme değil **overlay** olsun (göz hassasiyeti olan
  kullanıcı için kontrastı bozmasın); otomatik OP/ED atlama ayar ekranında
  "her zaman sor" seçeneği taşısın (epilepsi/anksiyete: beklenmedik jump).
- Renk tek başına anlam taşımaz: her mood ikon + etiketle gelir.

## 6. v2'ye Taşınma Notları

- Mood eşlemesi site API'sindeki tür/tag'lerle kurulur → yeni backend gerekmez,
  `api.rs` yetenekli; oturum ağırlıkları `~/.local/share/animecix/session.json`.
- Sosyal feed MVP'de **yalnızca yerel** (kullanıcının kendi cihazı + Discord link'i);
  sunucu tarafı arkadaş sistemi site API'si genişlemeden vaat edilmez.
- Mockup bu spec'e göre yeniden yazılacak: `mockup-mood.html`.
