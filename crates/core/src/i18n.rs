//! Interface language. Turkish strings in the source are the keys; the English table translates them.
//! `auto` follows the system language: Turkish systems get Turkish, everything else English.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Tr,
    En,
}

static CURRENT: AtomicU8 = AtomicU8::new(1);

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 0 {
        Lang::Tr
    } else {
        Lang::En
    }
}

pub fn set(l: Lang) {
    CURRENT.store(if l == Lang::Tr { 0 } else { 1 }, Ordering::Relaxed);
}

/// Language for a locale tag such as `tr_TR.UTF-8`, `tr-TR` or `en-US`.
pub fn from_locale(tag: &str) -> Lang {
    if tag.trim().to_lowercase().starts_with("tr") {
        Lang::Tr
    } else {
        Lang::En
    }
}

/// Resolve the `language` setting (`auto`, `en`, `tr`) and make it current.
pub fn apply(setting: &str, system_locale: Option<String>) -> Lang {
    let l = match setting {
        "tr" => Lang::Tr,
        "en" => Lang::En,
        _ => system_locale.map(|s| from_locale(&s)).unwrap_or(Lang::En),
    };
    set(l);
    l
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| EN.iter().copied().collect())
}

/// Translate a Turkish source string to the current language.
pub fn tr(s: &'static str) -> &'static str {
    if current() == Lang::Tr {
        return s;
    }
    table().get(s).copied().unwrap_or(s)
}

pub static EN: &[(&str, &str)] = &[
    ("Seç / Taşı (V)", "Select / Move (V)"),
    ("Dikdörtgen (R)", "Rectangle (R)"),
    ("Elips (E)", "Ellipse (E)"),
    ("Çizgi (L)", "Line (L)"),
    ("Ok (A)", "Arrow (A)"),
    ("Kalem (P)", "Pen (P)"),
    ("Fosforlu kalem (H)", "Highlighter (H)"),
    ("Metin (T)", "Text (T)"),
    ("Numara (N)", "Number (N)"),
    ("Mozaik (M)", "Mosaic (M)"),
    ("Bulanıklaştır (B)", "Blur (B)"),
    ("Güç", "Strength"),
    ("Boyut", "Size"),
    ("Kalınlık", "Width"),
    ("Yazı", "Text"),
    ("Özel renk…", "Custom colour…"),
    ("Dolgu", "Fill"),
    ("Kesik çizgi", "Dashed"),
    ("Geri al (Ctrl+Z)", "Undo (Ctrl+Z)"),
    ("Yinele (Ctrl+Y)", "Redo (Ctrl+Y)"),
    ("Seçiliyi sil (Del)", "Delete selected (Del)"),
    ("Ekrana sabitle (F3 / Ctrl+P)", "Pin to screen (F3 / Ctrl+P)"),
    ("Metni tanı – OCR (O)", "Recognise text – OCR (O)"),
    ("Bu alanı video olarak kaydet (MP4 / GIF)", "Record this area as video (MP4 / GIF)"),
    ("Kaydet… (Ctrl+S)", "Save… (Ctrl+S)"),
    ("İptal (Esc)", "Cancel (Esc)"),
    ("Panoya kopyala (Enter)", "Copy to clipboard (Enter)"),
    ("Alanı seçmek için sürükle  ·  Ctrl+A tüm ekran  ·  C renk kopyala  ·  Esc iptal", "Drag to select an area  ·  Ctrl+A whole screen  ·  C copy colour  ·  Esc cancel"),
    ("C kopyala · Shift biçim", "C copy · Shift format"),
    ("PinShot zaten çalışıyor", "PinShot is already running"),
    ("macOS: Sistem Ayarları › Gizlilik › Ekran Kaydı'nda PinShot'a izin verin, sonra yeniden başlatın.", "macOS: allow PinShot in System Settings › Privacy › Screen Recording, then restart it."),
    ("Ekran yakalanamadı (görüntü boş). İzinleri kontrol edin.", "Could not capture the screen (blank image). Check permissions."),
    ("Ekran yakalanamadı", "Could not capture the screen"),
    ("Önce bir bölge yakalayın", "Capture an area first"),
    ("Son bölge geçersiz", "The last area is no longer valid"),
    ("Tam ekran", "Full screen"),
    ("Son bölge", "Last area"),
    ("panoya kopyalandı", "copied to clipboard"),
    ("panoya kopyalanamadı", "could not copy to clipboard"),
    ("Panoya kopyalandı", "Copied to clipboard"),
    ("Panoya kopyalanamadı", "Could not copy to clipboard"),
    ("kaydedildi", "saved"),
    ("kaydedilemedi", "could not save"),
    ("Kaydedildi", "Saved"),
    ("Kaydedilemedi", "Could not save"),
    ("Kaydet", "Save"),
    ("Renk kopyalandı", "Colour copied"),
    ("Panoda sabitlenecek görsel veya metin yok", "There is no image or text on the clipboard to pin"),
    ("Açılamadı", "Could not open"),
    ("Metin bulunamadı", "No text found"),
    ("Metin tanınıyor…", "Recognising text…"),
    ("Metin kopyalandı", "Text copied"),
    ("OCR kullanılamıyor", "OCR is not available"),
    ("Metin bulunamadı.", "No text found."),
    ("OCR başarısız", "OCR failed"),
    ("karakter – panoya kopyalandı", "characters – copied to clipboard"),
    ("Bölge yakala", "Capture area"),
    ("Gecikmeli yakala", "Delayed capture"),
    ("3 saniye", "3 seconds"),
    ("5 saniye", "5 seconds"),
    ("10 saniye", "10 seconds"),
    ("Tam ekran yakala", "Capture full screen"),
    ("Son bölgeyi tekrarla", "Repeat last area"),
    ("Ekranı video olarak kaydet (GIF / MP4)…", "Record screen as video (GIF / MP4)…"),
    ("Kaydı durdur", "Stop recording"),
    ("Panodan sabitle", "Pin from clipboard"),
    ("Dosyadan sabitle…", "Pin from file…"),
    ("Tüm sabitlenenleri kapat", "Close all pinned"),
    ("Kayıt klasörünü aç", "Open save folder"),
    ("Ayarlar…", "Settings…"),
    ("Hakkında", "About"),
    ("Çıkış", "Quit"),
    ("PinShot – kayıt sürüyor", "PinShot – recording"),
    ("Global kısayollar başlatılamadı. Tray menüsünü veya 'pinshot --capture' komutunu kullanın.", "Global shortcuts could not start. Use the tray menu or the 'pinshot --capture' command."),
    ("saniye sonra yakalanacak", "seconds until capture"),
    ("Ses cihazları taranamadı.", "Audio devices could not be scanned."),
    ("Video kaydı için ffmpeg bulunamadı.", "ffmpeg was not found: it is needed for video recording."),
    ("Kayıt için daha büyük bir alan seçin", "Select a larger area to record"),
    ("Kayıt başlatılamadı", "Could not start recording"),
    ("Kayıt penceresi açılamadı", "Could not open the recording window"),
    ("Kayıt kaydediliyor…", "Saving the recording…"),
    ("Kayıt başarısız", "Recording failed"),
    ("Kayıt hazır", "Recording ready"),
    ("dosya panoya kopyalandı", "file copied to clipboard"),
    ("panonuza dokunulmadı", "your clipboard was left alone"),
    ("Bilgisayar hedef kare hızını yakalayamadı:", "The computer could not keep up with"),
    ("ortalama", "average"),
    ("Klasörü açmak için tıkla.", "Click to open the folder."),
    ("Durdur", "Stop"),
    ("Seçili alanı kaydet", "Record selected area"),
    ("Ekranı kaydet", "Record screen"),
    ("Biçim", "Format"),
    ("Kare hızı", "Frame rate"),
    ("Orijinal", "Original"),
    ("Maksimum kalite (4:4:4 renk)", "Maximum quality (4:4:4 colour)"),
    ("Sistem sesi", "System sound"),
    ("Mikrofon", "Microphone"),
    ("Varsayılan mikrofon", "Default microphone"),
    ("İptal", "Cancel"),
    ("Tüm ekranı kaydet", "Record whole screen"),
    ("Kaydı başlat", "Start recording"),
    ("Kopyala", "Copy"),
    ("Kaydet…", "Save…"),
    ("Metni tanı (OCR)", "Recognise text (OCR)"),
    ("Opaklık", "Opacity"),
    ("Dönüştür", "Transform"),
    ("Yatay çevir", "Flip horizontally"),
    ("Dikey çevir", "Flip vertically"),
    ("Saat yönünde döndür", "Rotate clockwise"),
    ("Saat yönünün tersine döndür", "Rotate counter-clockwise"),
    ("Konumu kilitle", "Lock position"),
    ("Kenarlık", "Border"),
    ("Kapat", "Close"),
    ("Tüm pinleri kapat", "Close all pins"),
    ("Sabitlenecek görseller", "Images to pin"),
    ("Görseller", "Images"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_pick_a_language() {
        assert_eq!(from_locale("tr_TR.UTF-8"), Lang::Tr);
        assert_eq!(from_locale("tr-TR"), Lang::Tr);
        assert_eq!(from_locale("en-US"), Lang::En);
        assert_eq!(from_locale("de_DE"), Lang::En);
        assert_eq!(from_locale(""), Lang::En);
    }

    #[test]
    fn setting_overrides_the_system_language() {
        assert_eq!(apply("auto", Some("tr_TR".into())), Lang::Tr);
        assert_eq!(apply("auto", None), Lang::En);
        assert_eq!(apply("en", Some("tr_TR".into())), Lang::En);
        assert_eq!(apply("tr", Some("en_US".into())), Lang::Tr);
    }

    #[test]
    fn translates_only_in_english_and_falls_back_to_the_key() {
        set(Lang::En);
        assert_eq!(tr("Elips (E)"), "Ellipse (E)");
        assert_eq!(tr("unknown string"), "unknown string");
        set(Lang::Tr);
        assert_eq!(tr("Elips (E)"), "Elips (E)");
        set(Lang::En);
    }

    #[test]
    fn table_has_no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in EN {
            assert!(seen.insert(*k), "duplicate key {k}");
        }
    }
}
