# Design Style: Cinematic / Streaming (Netflix-inspired, AnimeciX-adapted)

> Bu dosya bir prompt/spec'tir: herhangi bir ajan/geliştirici bunu alıp AnimeciX v2
> (Tauri + Svelte) arayüzünü bu tasarım diline göre inşa edebilir.
> Kopyalanan şey **düzen ve etkileşim dilidir**; Netflix kelime markası, "N" logo,
> kurumsal kırmızısı ve hiçbir görsel asset'i kullanılmaz. Aksan rengi markamıza aittir.

## 1. Design Philosophy

Bu stil **kamera-front (camera-front)** estetiğidir: arayüzün kendisi kararır,
içerik (poster/kapak) parlar. Arayüz "koleksiyoncu vitrini" gibi davranır —
sana içerik hakkında konuşmaz, içeriği **konuşturur**. Koyu, sabırlı, sinematik.

### Visual DNA
* **Core Signature**: Ufuk kaydırmalı içerik rayları (rails); kart hover'da
  gecikmeli olarak büyür, komşuları iterek yer açar ve ön plana çıkar.
* **İçerik kraldır**: UI asla içerikten daha parlak değildir; tüm yüzeyler
  #141414-altı gri skalada kalır, renk sadece içerik ve tek aksanda vardır.
* **Kenarlar ekranın dışına akar**: raylar viewport'a sağdan temas ederek sonlanır
  (padding yok) — "daha da var" hismi verir.
* **Gradyanla gömme**: hero'nun altına ve rayların kenarlarına dikey/yatay koyu
  gradyanlar koyarak içerik UI'a değil, karanlığa çözülür.
* **Typography**: düz, geniş, hafif kalın sans (Inter/Geist/Archivo). Karakterli
  serif YASAK — başlıklar bile "altyazı" gibi davranır.

### Design Principles
* **Vibe**: sinema salonu, cuma gecesi, "bir bölüm daha".
* **Kural 1**: Ekranın %70'i içerik görseli olmalı; chrome (nav, başlık, buton) minimum.
* **Kural 2**: Yatay keşif > diay scroll. Dikey scroll sadece "içerik" akışıdır, konteyner değil.
* **Kural 3**: Hover = önizleme. Dokunma = bilgi. Tık = oynat. Üçü karışmaz.
* **Kural 4**: İlerleme her yerde görünür: yarım kalan her şey ray'da progress bar taşır.
* **Z-hiyerarşisi**: büyüyen kart z-50'ye çıkar, komşular `translateX` ile yumuşak itilir,
  ray container'ı `overflow: visible` + mask/clip ile kesilir.

## 2. Design Token System

### Renkler (dark-first; tek açık varyant yok)
* `bg`: `#0B0B0E` (salon karartması — saf siyah değil, mavimsi koyu)
* `bg-elevated`: `#16161A`
* `bg-card`: `#1D1D23`
* `fg`: `#F2F2F2` (beyaz değil — göz yorulmasın)
* `fg-muted`: `#9A9AA3`
* `fg-faint`: `#5D5D66`
* `accent`: `#E23B3B` ( AnimeciX kırmızısı — Netflix kırmızısı DEĞİL, ondan daha az doygun/kutuplaştırıcı; markan kendi kırmızını seç, öneri aşağıda*)
* `accent-hover`: `#F25555`
* `success`: `#3FBF6C` (indirme tamamlandı)
* `overlay`: `rgba(0,0,0,0.6)` (modal arkası, hero gradient'leri)
* `ring`: `rgba(242,242,242,0.8)` (odak halkası — asla accent rengi değil, beyaz)
* *Not: accent için alternatifler: hardal `#D9A441` (moody) veya turuncu-kırmızı `#E2593B`.
  Karar: tek accent, her yerde o; ikinci vurgu rengi yok.

### Tipografi
* **Tüm arayüz**: **Archivo** (veya Inter) — başlıklar 700-800, gövde 400-500.
* **Hero başlığı**: 56-72px, 800 weight, sıkı tracking (`-0.02em`), üstüne kopyalanmış
  logo-tipografi hissi vermez; düz puntolu, sinematik afiş başlığı gibi.
* **Ray başlıkları**: 18-20px, 700, `fg`, yanında sağa oklu "hepsi ›" ghost butonu.
* **Meta/rozet yazıları**: 12px, 500, uppercase değil, `fg-muted`.
* Ölçek: 1.2 minor-third. Kart içi yazılar asla 12px altına inmez.

### Radius & Şekil
* Kartlar: `8px` (cömert değil, keskin değil — ekrandaki onlarca kart için sakin).
* Butonlar: `4px` (aksilik yapma: pill DEĞİL — Netflix-yolu dikdörtgen butondur).
* Modallar/panels: `8-12px`.
* Rozetler: `3px`, sola yapışık dikey şerif (maturity tarzı: `border-left: 3px solid fg-muted`).

### Gölgeler & Derinlik
* Büyüyen kart: `0 12px 40px rgba(0,0,0,0.7)` — siyah, sertçe, "sahne ışığı" değil "gölge kutu".
* Alt bilgi çubukları: `0 -4px 24px rgba(0,0,0,0.6)`.
* Grain/noise overlay YOK (Organic stilinden fark burda: yüzeyler düz ve karanlıktır).

## 3. Component Stylings

### Hero Billboard (her ray'ın üstündeki tam-genişlik vitrin)
* Yükseklik: `min(72vh, 720px)`; arka plan: 16:9 episode still, sağa hizalı,
  `object-fit: cover`; alt %40'ı ve sağ kenarı koyu gradyanla erir.
* İçerik sol-altta: yapım adı (büyük), 1 satır özet (max 280 karakter),
  meta satırı (yıl · bölüm/Sezon · süre · fansub grubu), buton çifti:
  **[▶ Devam Et]** (solid accent/white) + **[+ Listem]** (outline, translucent grey).
* Sağ alt köşede ses seviyesi/altyazı rozeti gibi pasif ikonlar; otomatik 8s'de
  sessiz preview döner (veya rotatif spotlight olarak favorilerden beslenir).
* Scroll'da billboard yukarı kayarken parlaklığı azalır (sticky + opacity-yaklaşık).

### Ray (yatay içerik şeridi) — sistemin çekirdeği
* Kart ölçüleri: yatay kart `264×148` (16:9, devam-et ve trend için);
  dikey poster `150×225` (2:3, "Koleksiyonum"/takip için) — anime için ana format poster.
* Hover (300ms gecikme + 500ms animasyon): kart `scale(1.38)`, z-50, y-4px;
  yanlarındaki kartlar `translateX(±10%)` ile kaçar; kartın altında `bg-elevated`
  bilgi paneli açılır: [▶ oynat] [+] [beğeni] + rozet satırı (bölüm no, süre, %alaka).
* Ray başına 12-18 kart; taşarsa sağa-sola chevron'lar (`opacity:0 → hover ray'da 1`).
* Devam-Ediyorsun ray'ı: kart altında `2px` kırmızı progress + "bölüme kaldığın yerden devam et" linki.
* "Top 10" varyantı: kartın solunda dev, outline-only, boş rakam (font-stroke) `1..10`.

### Navigasyon
* Üst bar: `transparent → #0B0B0E` (scroll 40px'de tam opak, 200ms). Sol: logo (düz
  tipografik, accent renkli). Sağ: arama (ikon→açılır input), bildirim, avatar.
* Scroll'a duyarlı olmak tek numarası; geri kalanı hep sabit ve sade.
* Yan menü YOK (organik kalabalık yapar); mobilde üst bar hamburger'e düşer.

### Butonlar
* Primary: `bg-white text-black` veya accent; ikon+label (▶ Oynat / İndir).
* Ghost: `rgba(90,90,90,0.4)` bg + `backdrop-blur`, hover `0.6`.
* Ikon-buton: `40px` yuvarlak değil, `4px` radius, `fg-muted → fg`.

### Detay Sayfası / Modal
* Tam genişlik hero (billboard'ın küçüğü) + altta: bölüm listesi (dikey),
  benzer içerik grid'i, "Hakkında" metası.
* Bölüm satırı: `88×50` thumb, bölüm no, ad, süre, özet-tek-satır, hover'da sağda ▶.
* Modal ise: üstten kayan, `max-w-4xl`, backdrop fade; tam sayfaysa router'a bağlı derin link.

### Oynatıcı (player)
* Kontroller varsayılan gizli; mouse move ile `overlay` gradient + kontroller.
* Kontrol barı: alt, `rgba(0,0,0,0.8) → transparent` gradient; timeline ince
  (`3px → hover 5px`), buffered segmentler `rgba(255,255,255,0.3)`, tamamlanan accent.
* Timeline üstünde aniskip OP/ED segmentleri **görsel olarak boyanır**: OP aralığı
  sarımsı, ED morumsu, hover'da "İntroyu Atla →" pill'i.
* Sağ üst: kaynak/kayıt/hız menüsü (tek ⚙); sol üst: bölüm seçici + geri.
* Otomatik geçiş: bölüm bitince 5s geri sayım tam-ekran overlay ("Sonraki bölüm" + thumb).

### Arama & Keşif
* Arama odaklanınca ekran kararır, tam-ekran arama paneli açılır (sorgu büyük yazılır,
  altındaki 6-8 öneri ray'la anında değişir — "query preview" numarasi).
* Filtreler: genres/tür/yıl/durum, çoklu seçilebilir chip'ler (accent dolu/bombalı değil —
  `bg-card border` + seçili `fg + border-accent`).

### İndirme Yöneticisi
* Ray olarak değil, yan çekmece (`right-drawer`, `bg-elevated`) — çünkü Netflix'in
  "download"unda hız/progress olmaz; bu yüzden kendi pattern'in: listedeki her
  anime satırında sağda `↓%` rozeti; çekmece aktif kuyruğu gösterir.

## 4. Layout & Spacing
* Tam genişlik esnek; max-konteyner `1920px`, ray'lar kenara yapışık (`padding-inline: clamp(16px, 4vw, 64px)` —
  sadece ilk kart görünür başlangıçta; kaydırınca kenara kadar akar).
* Ray arası dikey ritim: `48px` (Organic'in py-32'si burada boğulur).
* Grid: `repeat(auto-fill, minmax(160px, 1fr))` poster grid; gap `12px`.
* Breakpoint'ler: 640 / 1024 / 1440. 1024 altında billboard başlık boyutu bir kademe iner.

## 5. Non-Genericness (AnimeciX'e özgü dokunuşlar)
* Poster hover'ında sessiz OP snippet'i değil — fansub grubu logosu + bölüm durum şeridi.
* Maraton modu: "sıradaki" kartı ray'ın ucunda her zaman görünür ve nabız gibi atar.
* Sezon ayracı: uzun serilerde ray içi sticky mini-sezon etiketleri.
* Yayın takvimi, Netflix'in "Coming Soon" ray'ının yaşayan hali: bugünün günü vurgulu.

## 6. Effects & Animation
* Tempo: hızlı ve kendinden emin. `150-250ms`, ease-out (`cubic-bezier(0.2, 0, 0, 1)`).
  Yumuşak yaylanma yok, "organik" salınım yok.
* Kart açılma: `transform` + `will-change`, gecikme-first (300ms intent delay).
* Sayfa geçişleri: fade 150ms; hero billboard parallax (scroll'a bağlı `translateY(0.15×)`).
* Odak/erişilebilirlik: klavye ile ray gezinme (ok tuşları), `:focus-visible` ring'i beyaz.

## 7. Icons
* `lucide` seti (lucide-svelte), stroke 1.5-2, boyut 20-24, renk her zaman `fg-muted → fg`;
  ASLA accent renkli ikon (renk kotası yalnızca accent'e ait).

## 8. Accessibility
* Metin kontrastları `fg/#0B0B0E ≥ 13:1`, `fg-muted ≥ 5.5:1` (AAA/AA).
* Tüm ray'lar klavyeyle dolaşılabilir (roving tabindex); hover-expand paneli
  `prefers-reduced-motion`'da anında açılır, scale yapmaz.
* Oynatıcı kontrolleri odaklanabilir ve tam-ekran'da `Tab` ile erişilebilir;
  otomatik geçiş overlay'inde `İptal` odakta başlar.
* Dokunmatikte hover-expand yerine ilk dokunuş = select, ikincisi = play.

## 9. Responsive
* Mobil: billboard `50vh` olur, başlık altına taşınır; ray'lar kart genişliği `120px` poster;
  hover-expand kalkar → tap-to-detail modal.
* Oynatıcı tam ekran; timeline dokunmatik için `12px`.
* 320-480px arası: meta satırları tek satıra daralır.

## Uygulama Notları (teknik)
* Frame: Tauri + Svelte; ray sanal-scroll yerine pencere-bazlı `createViewport` + lazy img.
* Kart imge srcset: 1x/2x poster; LQIP = 24px blurhash değil, `bg-card` boş rengi (hız).
* Rewind: "Devam Ediyorsun" ray'ı backend progress sync'i ile beslenir (`api.rs` zaten tutuyor).
* Player: `<video>` + hls.js + localhost proxy (kaynak header enjeksiyonu).
