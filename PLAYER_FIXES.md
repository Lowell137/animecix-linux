# Player & Kalite — düzeltme sırası (2026-09-23)

Önem sırasına göre. Altı çizili = daha önce yapılıp düzgün çalışmayan/eksik.

1. **Ortadaki oynat/duraklat butonuna tıklayınca çalışmıyor** (space çalışıyor).
   → Neden: ortadaki flaş (flash) katmanı butonların ÜSTÜNDE eklenmiş; tıklamayı yutuyor.
   → Flaş katmanını butonlardan ÖNCE (alta) ekle. Butonlar en üstte kalsın.

2. **Kitsune düzeni — buton yerleşimi:**
   - Önceki bölüm → ekranın EN SOL ucu (dikey ortada).
   - Sonraki bölüm → ekranın EN SAĞ ucu (dikey ortada).
   - Ortada sadece: [-10] [oynat/duraklat] [+10].
   - İkonlar BÜYÜSÜN; arka planları olsun ama **BEYAZ + ŞEFFAF** (yarı saydam dairesel).

3. **İlerleme (seek) çubuğu en alta inmiş; ÜSTTE olmalı.**
   - Alt küme sırası: önce seek satırı (süre · bar · süre), altında başlık+hız+kalite+satırı.

4. **Player'dan SAĞ ÜST kontrol butonlarını (pencere: küçült/büyüt/kapat) kaldır.**
   - Üstte sadece solda "geri" kalsın. Tam ekran: F + çift tık.

5. **Kalite menüsü "regular" yazıyor — hangisi 720p belli değil.**
   - Veri gerçeği: API'nin `videos[].quality` alanı "regular" (fansub kalite kademesi), ÇÖZÜNÜRLÜK DEĞİL.
   - O yüzden menüde "regular"ı kaldır; host + (Önerilen/yedek) göster; gerçek çözünürlüğü yalnızca şu an oynayan kaynak için (video_height) göster.
   - Kullanıcıya dürüst cevap: kaynaklar 720/1080 olarak etiketli değil.

6. **Siyah ekran (#38)** — hâlâ açık, ayrı iş (bu turda dokunulmuyor).

Durum: 1,2,3,4,5 bu turda yapılacak. 6 ayrı.

## DURUM (2026-09-23): 1–5 YAPILDI, build+test yeşil (102 test), binary 10:26. GUI doğrulanmadı.
- 1 ✓ flash katmanı butonların altına alındı (tıklama artık butona gidecek).
- 2 ✓ prev sol uç / next sağ uç (ayrı revealer, kromla gizlenir); ortada [-10][play][+10]; `.player-fab` beyaz yarı saydam daire, play 64px.
- 3 ✓ seek satırı üste, başlık+hız+kalite+ses alta.
- 4 ✓ sağ üst pencere butonları kaldırıldı (show_end_title_buttons=false); sadece solda "geri".
- 5 ✓ "regular" kalktı; çözünürlük yalnızca gerçekse veya şu an oynayan kaynakta (mpv video_height) gösteriliyor.
- 6 (siyah ekran) ayrı iş, dokunulmadı.
