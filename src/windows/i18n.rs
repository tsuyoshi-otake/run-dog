//! Tray menu copy follows the Windows display language.

use std::ptr;

use windows_sys::Win32::{
    Globalization::GetUserDefaultUILanguage,
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

/// GitHub Pages product site opened from About.
pub const ABOUT_PAGE_URL: &str = "https://tsuyoshi-otake.github.io/run-dog/";

/// Languages the tray menu can render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiLanguage {
    Japanese,
    English,
    Korean,
    Chinese,
    ChineseTraditional,
    Vietnamese,
    French,
    German,
    Spanish,
    Russian,
    Italian,
    Thai,
}

impl UiLanguage {
    /// Maps a Win32 LANGID to a supported menu language. Unknown IDs use English.
    #[must_use]
    pub const fn from_langid(langid: u16) -> Self {
        // Traditional Chinese: Taiwan, Hong Kong, Macau.
        match langid {
            0x0404 | 0x0C04 | 0x1404 => return Self::ChineseTraditional,
            _ => {}
        }
        match primary_langid(langid) {
            0x11 => Self::Japanese,
            0x12 => Self::Korean,
            0x04 => Self::Chinese,
            0x2A => Self::Vietnamese,
            0x0C => Self::French,
            0x07 => Self::German,
            0x0A => Self::Spanish,
            0x19 => Self::Russian,
            0x10 => Self::Italian,
            0x1E => Self::Thai,
            _ => Self::English,
        }
    }

    /// BCP-47 tag used by the GitHub Pages `?lang=` switcher.
    #[must_use]
    pub const fn pages_code(self) -> &'static str {
        match self {
            Self::Japanese => "ja",
            Self::English => "en",
            Self::Korean => "ko",
            Self::Chinese => "zh",
            Self::ChineseTraditional => "zh-TW",
            Self::Vietnamese => "vi",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Russian => "ru",
            Self::Italian => "it",
            Self::Thai => "th",
        }
    }

    #[must_use]
    pub fn about_url(self) -> String {
        match self {
            Self::Japanese => ABOUT_PAGE_URL.to_owned(),
            other => format!("{ABOUT_PAGE_URL}?lang={}", other.pages_code()),
        }
    }

    #[must_use]
    pub const fn menu(self) -> MenuText {
        match self {
            Self::Japanese => JA,
            Self::English => EN,
            Self::Korean => KO,
            Self::Chinese => ZH,
            Self::ChineseTraditional => ZH_HANT,
            Self::Vietnamese => VI,
            Self::French => FR,
            Self::German => DE,
            Self::Spanish => ES,
            Self::Russian => RU,
            Self::Italian => IT,
            Self::Thai => TH,
        }
    }
}

/// Reads the current Windows UI language once when the menu is built.
#[must_use]
pub fn current() -> UiLanguage {
    UiLanguage::from_langid(unsafe { GetUserDefaultUILanguage() })
}

/// Opens the GitHub Pages site in the default browser, matching the UI language.
pub fn open_about_page() {
    let url = current().about_url();
    let operation = wide("open");
    let path = wide(&url);
    let _ = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

const fn primary_langid(langid: u16) -> u16 {
    langid & 0x03FF
}

/// All user-visible tray menu and balloon strings for one language.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuText {
    pub display: &'static str,
    pub display_dog: &'static str,
    pub display_cpu: &'static str,
    pub display_memory: &'static str,
    pub display_gpu: &'static str,
    pub display_claude_5h: &'static str,
    pub display_claude_week: &'static str,
    pub display_fable_week: &'static str,
    pub display_codex_5h: &'static str,
    pub display_codex_week: &'static str,
    pub theme: &'static str,
    pub theme_system: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub animation_speed: &'static str,
    pub fps_10: &'static str,
    pub fps_20: &'static str,
    pub fps_30: &'static str,
    pub fps_40: &'static str,
    pub startup_on: &'static str,
    pub startup_off: &'static str,
    pub pin_on: &'static str,
    pub pin_off: &'static str,
    pub rescan_month: &'static str,
    pub check_updates: &'static str,
    pub checking_updates: &'static str,
    pub up_to_date: &'static str,
    pub install_update: &'static str,
    pub check_again: &'static str,
    pub downloading: &'static str,
    pub starting_installer: &'static str,
    pub retry_update: &'static str,
    pub update_failed: &'static str,
    pub about: &'static str,
    pub exit: &'static str,
    pub balloon_startup_on: &'static str,
    pub balloon_startup_off: &'static str,
    pub balloon_rescan_started: &'static str,
    pub balloon_rescan_finished: &'static str,
    pub balloon_up_to_date: &'static str,
    pub balloon_available: &'static str,
    pub balloon_check_failed: &'static str,
}

impl MenuText {
    #[must_use]
    pub const fn startup(self, enabled: bool) -> &'static str {
        if enabled {
            self.startup_on
        } else {
            self.startup_off
        }
    }

    #[must_use]
    pub const fn pinned_flyout(self, enabled: bool) -> &'static str {
        if enabled {
            self.pin_on
        } else {
            self.pin_off
        }
    }

    #[must_use]
    pub fn with_version(self, template: &'static str, version: &str) -> String {
        template.replace("{version}", version)
    }
}

const JA: MenuText = MenuText {
    display: "トレイ表示",
    display_dog: "犬（CPU）",
    display_cpu: "CPU",
    display_memory: "メモリ",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5時間",
    display_claude_week: "Claude 週",
    display_fable_week: "Fable 週",
    display_codex_5h: "Codex 5時間",
    display_codex_week: "Codex 週",
    theme: "テーマ",
    theme_system: "システム",
    theme_light: "ライト",
    theme_dark: "ダーク",
    animation_speed: "アニメーション速度の上限",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "スタートアップで起動: オン",
    startup_off: "スタートアップで起動: オフ",
    pin_on: "モニターカードを固定: オン",
    pin_off: "モニターカードを固定: オフ",
    rescan_month: "今月の利用状況を再スキャン",
    check_updates: "更新を確認",
    checking_updates: "更新を確認しています...",
    up_to_date: "最新版です",
    install_update: "RunDog v{version} をインストール",
    check_again: "再確認",
    downloading: "RunDog v{version} をダウンロードしています...",
    starting_installer: "インストーラーを起動しています...",
    retry_update: "更新の確認を再試行",
    update_failed: "更新を完了できませんでした",
    about: "RunDog について",
    exit: "終了",
    balloon_startup_on: "スタートアップで起動はオンです。",
    balloon_startup_off: "スタートアップで起動はオフです。",
    balloon_rescan_started: "今月の利用状況の再スキャンを開始しました。",
    balloon_rescan_finished: "今月の利用状況のスキャンが完了しました。",
    balloon_up_to_date: "RunDog は最新版です。",
    balloon_available: "RunDog v{version} が利用できます。",
    balloon_check_failed: "更新を確認できませんでした。",
};

const EN: MenuText = MenuText {
    display: "Tray icon",
    display_dog: "Dog (CPU)",
    display_cpu: "CPU",
    display_memory: "Memory",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5h",
    display_claude_week: "Claude week",
    display_fable_week: "Fable week",
    display_codex_5h: "Codex 5h",
    display_codex_week: "Codex week",
    theme: "Theme",
    theme_system: "System",
    theme_light: "Light",
    theme_dark: "Dark",
    animation_speed: "Maximum animation speed",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Launch at startup: On",
    startup_off: "Launch at startup: Off",
    pin_on: "Pin monitor card: On",
    pin_off: "Pin monitor card: Off",
    rescan_month: "Full scan: current month usage",
    check_updates: "Check for updates",
    checking_updates: "Checking for updates...",
    up_to_date: "RunDog is up to date",
    install_update: "Install RunDog v{version}",
    check_again: "Check again",
    downloading: "Downloading RunDog v{version}...",
    starting_installer: "Starting installer...",
    retry_update: "Retry update check",
    update_failed: "Update could not be completed",
    about: "About",
    exit: "Exit",
    balloon_startup_on: "Launch at startup is on.",
    balloon_startup_off: "Launch at startup is off.",
    balloon_rescan_started: "Full scan of current month usage started.",
    balloon_rescan_finished: "Current month usage scan complete.",
    balloon_up_to_date: "RunDog is up to date.",
    balloon_available: "RunDog v{version} is available.",
    balloon_check_failed: "Could not check for updates.",
};

const KO: MenuText = MenuText {
    display: "트레이 아이콘",
    display_dog: "개 (CPU)",
    display_cpu: "CPU",
    display_memory: "메모리",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5시간",
    display_claude_week: "Claude 주간",
    display_fable_week: "Fable 주간",
    display_codex_5h: "Codex 5시간",
    display_codex_week: "Codex 주간",
    theme: "테마",
    theme_system: "시스템",
    theme_light: "밝게",
    theme_dark: "어둡게",
    animation_speed: "최대 애니메이션 속도",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "시작 시 실행: 켜짐",
    startup_off: "시작 시 실행: 꺼짐",
    pin_on: "모니터 카드 고정: 켜짐",
    pin_off: "모니터 카드 고정: 꺼짐",
    rescan_month: "이번 달 사용량 전체 스캔",
    check_updates: "업데이트 확인",
    checking_updates: "업데이트를 확인하는 중...",
    up_to_date: "최신 버전입니다",
    install_update: "RunDog v{version} 설치",
    check_again: "다시 확인",
    downloading: "RunDog v{version} 다운로드 중...",
    starting_installer: "설치 프로그램 시작 중...",
    retry_update: "업데이트 확인 다시 시도",
    update_failed: "업데이트를 완료할 수 없습니다",
    about: "정보",
    exit: "종료",
    balloon_startup_on: "시작 시 실행이 켜졌습니다.",
    balloon_startup_off: "시작 시 실행이 꺼졌습니다.",
    balloon_rescan_started: "이번 달 사용량 전체 스캔을 시작했습니다.",
    balloon_rescan_finished: "이번 달 사용량 스캔이 완료되었습니다.",
    balloon_up_to_date: "RunDog는 최신 버전입니다.",
    balloon_available: "RunDog v{version}을(를) 사용할 수 있습니다.",
    balloon_check_failed: "업데이트를 확인할 수 없습니다.",
};

const ZH: MenuText = MenuText {
    display: "托盘图标",
    display_dog: "狗（CPU）",
    display_cpu: "CPU",
    display_memory: "内存",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 小时",
    display_claude_week: "Claude 每周",
    display_fable_week: "Fable 每周",
    display_codex_5h: "Codex 5 小时",
    display_codex_week: "Codex 每周",
    theme: "主题",
    theme_system: "系统",
    theme_light: "浅色",
    theme_dark: "深色",
    animation_speed: "动画速度上限",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "开机启动: 开",
    startup_off: "开机启动: 关",
    pin_on: "固定监视卡片: 开",
    pin_off: "固定监视卡片: 关",
    rescan_month: "完整扫描本月用量",
    check_updates: "检查更新",
    checking_updates: "正在检查更新...",
    up_to_date: "已是最新版本",
    install_update: "安装 RunDog v{version}",
    check_again: "再次检查",
    downloading: "正在下载 RunDog v{version}...",
    starting_installer: "正在启动安装程序...",
    retry_update: "重试检查更新",
    update_failed: "无法完成更新",
    about: "关于",
    exit: "退出",
    balloon_startup_on: "开机启动已打开。",
    balloon_startup_off: "开机启动已关闭。",
    balloon_rescan_started: "已开始完整扫描本月用量。",
    balloon_rescan_finished: "本月用量扫描已完成。",
    balloon_up_to_date: "RunDog 已是最新版本。",
    balloon_available: "RunDog v{version} 可供安装。",
    balloon_check_failed: "无法检查更新。",
};

const ZH_HANT: MenuText = MenuText {
    display: "工作列圖示",
    display_dog: "狗（CPU）",
    display_cpu: "CPU",
    display_memory: "記憶體",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 小時",
    display_claude_week: "Claude 每週",
    display_fable_week: "Fable 每週",
    display_codex_5h: "Codex 5 小時",
    display_codex_week: "Codex 每週",
    theme: "主題",
    theme_system: "系統",
    theme_light: "淺色",
    theme_dark: "深色",
    animation_speed: "動畫速度上限",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "開機啟動: 開",
    startup_off: "開機啟動: 關",
    pin_on: "固定監視卡片: 開",
    pin_off: "固定監視卡片: 關",
    rescan_month: "完整掃描本月用量",
    check_updates: "檢查更新",
    checking_updates: "正在檢查更新...",
    up_to_date: "已是最新版本",
    install_update: "安裝 RunDog v{version}",
    check_again: "再次檢查",
    downloading: "正在下載 RunDog v{version}...",
    starting_installer: "正在啟動安裝程式...",
    retry_update: "重試檢查更新",
    update_failed: "無法完成更新",
    about: "關於",
    exit: "結束",
    balloon_startup_on: "開機啟動已打開。",
    balloon_startup_off: "開機啟動已關閉。",
    balloon_rescan_started: "已開始完整掃描本月用量。",
    balloon_rescan_finished: "本月用量掃描已完成。",
    balloon_up_to_date: "RunDog 已是最新版本。",
    balloon_available: "RunDog v{version} 可供安裝。",
    balloon_check_failed: "無法檢查更新。",
};

const VI: MenuText = MenuText {
    display: "Biểu tượng khay",
    display_dog: "Chó (CPU)",
    display_cpu: "CPU",
    display_memory: "Bộ nhớ",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 giờ",
    display_claude_week: "Claude tuần",
    display_fable_week: "Fable tuần",
    display_codex_5h: "Codex 5 giờ",
    display_codex_week: "Codex tuần",
    theme: "Giao diện",
    theme_system: "Hệ thống",
    theme_light: "Sáng",
    theme_dark: "Tối",
    animation_speed: "Tốc độ hoạt ảnh tối đa",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Khởi động cùng Windows: Bật",
    startup_off: "Khởi động cùng Windows: Tắt",
    pin_on: "Ghim thẻ giám sát: Bật",
    pin_off: "Ghim thẻ giám sát: Tắt",
    rescan_month: "Quét toàn bộ mức dùng tháng này",
    check_updates: "Kiểm tra cập nhật",
    checking_updates: "Đang kiểm tra cập nhật...",
    up_to_date: "RunDog đã là bản mới nhất",
    install_update: "Cài RunDog v{version}",
    check_again: "Kiểm tra lại",
    downloading: "Đang tải RunDog v{version}...",
    starting_installer: "Đang khởi chạy trình cài đặt...",
    retry_update: "Thử kiểm tra cập nhật lại",
    update_failed: "Không thể hoàn tất cập nhật",
    about: "Giới thiệu",
    exit: "Thoát",
    balloon_startup_on: "Đã bật khởi động cùng Windows.",
    balloon_startup_off: "Đã tắt khởi động cùng Windows.",
    balloon_rescan_started: "Đã bắt đầu quét toàn bộ mức dùng tháng này.",
    balloon_rescan_finished: "Đã quét xong mức dùng tháng này.",
    balloon_up_to_date: "RunDog đã là bản mới nhất.",
    balloon_available: "RunDog v{version} đã có.",
    balloon_check_failed: "Không thể kiểm tra cập nhật.",
};

const FR: MenuText = MenuText {
    display: "Icône de notification",
    display_dog: "Chien (CPU)",
    display_cpu: "CPU",
    display_memory: "Mémoire",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 h",
    display_claude_week: "Claude semaine",
    display_fable_week: "Fable semaine",
    display_codex_5h: "Codex 5 h",
    display_codex_week: "Codex semaine",
    theme: "Thème",
    theme_system: "Système",
    theme_light: "Clair",
    theme_dark: "Sombre",
    animation_speed: "Vitesse d'animation maximale",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Lancer au démarrage : activé",
    startup_off: "Lancer au démarrage : désactivé",
    pin_on: "Épingler la carte : activé",
    pin_off: "Épingler la carte : désactivé",
    rescan_month: "Analyse complète de l'usage du mois",
    check_updates: "Rechercher des mises à jour",
    checking_updates: "Recherche de mises à jour...",
    up_to_date: "RunDog est à jour",
    install_update: "Installer RunDog v{version}",
    check_again: "Vérifier à nouveau",
    downloading: "Téléchargement de RunDog v{version}...",
    starting_installer: "Démarrage de l'installateur...",
    retry_update: "Réessayer la recherche",
    update_failed: "La mise à jour n'a pas pu aboutir",
    about: "À propos",
    exit: "Quitter",
    balloon_startup_on: "Le lancement au démarrage est activé.",
    balloon_startup_off: "Le lancement au démarrage est désactivé.",
    balloon_rescan_started: "Analyse complète de l'usage du mois démarrée.",
    balloon_rescan_finished: "Analyse de l'usage du mois terminée.",
    balloon_up_to_date: "RunDog est à jour.",
    balloon_available: "RunDog v{version} est disponible.",
    balloon_check_failed: "Impossible de rechercher des mises à jour.",
};

const DE: MenuText = MenuText {
    display: "Infobereich",
    display_dog: "Hund (CPU)",
    display_cpu: "CPU",
    display_memory: "Speicher",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 Std.",
    display_claude_week: "Claude-Woche",
    display_fable_week: "Fable-Woche",
    display_codex_5h: "Codex 5 Std.",
    display_codex_week: "Codex-Woche",
    theme: "Design",
    theme_system: "System",
    theme_light: "Hell",
    theme_dark: "Dunkel",
    animation_speed: "Maximale Animationsgeschwindigkeit",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Beim Start ausführen: Ein",
    startup_off: "Beim Start ausführen: Aus",
    pin_on: "Monitorkarte anheften: Ein",
    pin_off: "Monitorkarte anheften: Aus",
    rescan_month: "Vollscan der Monatsnutzung",
    check_updates: "Nach Updates suchen",
    checking_updates: "Suche nach Updates...",
    up_to_date: "RunDog ist aktuell",
    install_update: "RunDog v{version} installieren",
    check_again: "Erneut prüfen",
    downloading: "RunDog v{version} wird heruntergeladen...",
    starting_installer: "Installer wird gestartet...",
    retry_update: "Updateprüfung wiederholen",
    update_failed: "Update konnte nicht abgeschlossen werden",
    about: "Info",
    exit: "Beenden",
    balloon_startup_on: "Starten mit Windows ist eingeschaltet.",
    balloon_startup_off: "Starten mit Windows ist ausgeschaltet.",
    balloon_rescan_started: "Vollscan der Monatsnutzung gestartet.",
    balloon_rescan_finished: "Scan der Monatsnutzung abgeschlossen.",
    balloon_up_to_date: "RunDog ist aktuell.",
    balloon_available: "RunDog v{version} ist verfügbar.",
    balloon_check_failed: "Updates konnten nicht geprüft werden.",
};

const ES: MenuText = MenuText {
    display: "Icono de notificación",
    display_dog: "Perro (CPU)",
    display_cpu: "CPU",
    display_memory: "Memoria",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 h",
    display_claude_week: "Claude semanal",
    display_fable_week: "Fable semanal",
    display_codex_5h: "Codex 5 h",
    display_codex_week: "Codex semanal",
    theme: "Tema",
    theme_system: "Sistema",
    theme_light: "Claro",
    theme_dark: "Oscuro",
    animation_speed: "Velocidad máxima de animación",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Iniciar con Windows: activado",
    startup_off: "Iniciar con Windows: desactivado",
    pin_on: "Fijar tarjeta del monitor: activado",
    pin_off: "Fijar tarjeta del monitor: desactivado",
    rescan_month: "Escaneo completo del uso de este mes",
    check_updates: "Buscar actualizaciones",
    checking_updates: "Buscando actualizaciones...",
    up_to_date: "RunDog está actualizado",
    install_update: "Instalar RunDog v{version}",
    check_again: "Volver a comprobar",
    downloading: "Descargando RunDog v{version}...",
    starting_installer: "Iniciando el instalador...",
    retry_update: "Reintentar la búsqueda",
    update_failed: "No se pudo completar la actualización",
    about: "Acerca de",
    exit: "Salir",
    balloon_startup_on: "El inicio con Windows está activado.",
    balloon_startup_off: "El inicio con Windows está desactivado.",
    balloon_rescan_started: "Se inició el escaneo completo del uso de este mes.",
    balloon_rescan_finished: "El escaneo del uso de este mes ha terminado.",
    balloon_up_to_date: "RunDog está actualizado.",
    balloon_available: "RunDog v{version} está disponible.",
    balloon_check_failed: "No se pudieron buscar actualizaciones.",
};

const RU: MenuText = MenuText {
    display: "Значок в трее",
    display_dog: "Собака (CPU)",
    display_cpu: "CPU",
    display_memory: "Память",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 ч",
    display_claude_week: "Claude за неделю",
    display_fable_week: "Fable за неделю",
    display_codex_5h: "Codex 5 ч",
    display_codex_week: "Codex за неделю",
    theme: "Тема",
    theme_system: "Как в системе",
    theme_light: "Светлая",
    theme_dark: "Тёмная",
    animation_speed: "Макс. скорость анимации",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Запуск при старте: вкл.",
    startup_off: "Запуск при старте: выкл.",
    pin_on: "Закрепить карточку: вкл.",
    pin_off: "Закрепить карточку: выкл.",
    rescan_month: "Полное сканирование за месяц",
    check_updates: "Проверить обновления",
    checking_updates: "Проверка обновлений...",
    up_to_date: "Установлена последняя версия",
    install_update: "Установить RunDog v{version}",
    check_again: "Проверить снова",
    downloading: "Загрузка RunDog v{version}...",
    starting_installer: "Запуск установщика...",
    retry_update: "Повторить проверку",
    update_failed: "Не удалось завершить обновление",
    about: "О программе",
    exit: "Выход",
    balloon_startup_on: "Запуск при старте включён.",
    balloon_startup_off: "Запуск при старте выключен.",
    balloon_rescan_started: "Начато полное сканирование за месяц.",
    balloon_rescan_finished: "Сканирование за месяц завершено.",
    balloon_up_to_date: "Установлена последняя версия RunDog.",
    balloon_available: "Доступна RunDog v{version}.",
    balloon_check_failed: "Не удалось проверить обновления.",
};

const IT: MenuText = MenuText {
    display: "Icona di notifica",
    display_dog: "Cane (CPU)",
    display_cpu: "CPU",
    display_memory: "Memoria",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 ore",
    display_claude_week: "Claude settimanale",
    display_fable_week: "Fable settimanale",
    display_codex_5h: "Codex 5 ore",
    display_codex_week: "Codex settimanale",
    theme: "Tema",
    theme_system: "Sistema",
    theme_light: "Chiaro",
    theme_dark: "Scuro",
    animation_speed: "Velocità massima animazione",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "Avvio automatico: sì",
    startup_off: "Avvio automatico: no",
    pin_on: "Fissa scheda monitor: sì",
    pin_off: "Fissa scheda monitor: no",
    rescan_month: "Scansione completa uso del mese",
    check_updates: "Controlla aggiornamenti",
    checking_updates: "Controllo aggiornamenti...",
    up_to_date: "RunDog è aggiornato",
    install_update: "Installa RunDog v{version}",
    check_again: "Controlla di nuovo",
    downloading: "Download di RunDog v{version}...",
    starting_installer: "Avvio del programma di installazione...",
    retry_update: "Riprova il controllo",
    update_failed: "Aggiornamento non completato",
    about: "Informazioni",
    exit: "Esci",
    balloon_startup_on: "Avvio automatico attivato.",
    balloon_startup_off: "Avvio automatico disattivato.",
    balloon_rescan_started: "Scansione completa dell'uso del mese avviata.",
    balloon_rescan_finished: "Scansione dell'uso del mese completata.",
    balloon_up_to_date: "RunDog è aggiornato.",
    balloon_available: "È disponibile RunDog v{version}.",
    balloon_check_failed: "Impossibile controllare gli aggiornamenti.",
};

const TH: MenuText = MenuText {
    display: "ไอคอนถาด",
    display_dog: "สุนัข (CPU)",
    display_cpu: "CPU",
    display_memory: "หน่วยความจำ",
    display_gpu: "GPU",
    display_claude_5h: "Claude 5 ชม.",
    display_claude_week: "Claude รายสัปดาห์",
    display_fable_week: "Fable รายสัปดาห์",
    display_codex_5h: "Codex 5 ชม.",
    display_codex_week: "Codex รายสัปดาห์",
    theme: "ธีม",
    theme_system: "ตามระบบ",
    theme_light: "สว่าง",
    theme_dark: "มืด",
    animation_speed: "ความเร็วแอนิเมชันสูงสุด",
    fps_10: "10 FPS",
    fps_20: "20 FPS",
    fps_30: "30 FPS",
    fps_40: "40 FPS",
    startup_on: "เริ่มเมื่อเปิดเครื่อง: เปิด",
    startup_off: "เริ่มเมื่อเปิดเครื่อง: ปิด",
    pin_on: "ปักหมุดการ์ดมอนิเตอร์: เปิด",
    pin_off: "ปักหมุดการ์ดมอนิเตอร์: ปิด",
    rescan_month: "สแกนการใช้งานเดือนนี้ทั้งหมด",
    check_updates: "ตรวจสอบการอัปเดต",
    checking_updates: "กำลังตรวจสอบการอัปเดต...",
    up_to_date: "RunDog เป็นเวอร์ชันล่าสุดแล้ว",
    install_update: "ติดตั้ง RunDog v{version}",
    check_again: "ตรวจสอบอีกครั้ง",
    downloading: "กำลังดาวน์โหลด RunDog v{version}...",
    starting_installer: "กำลังเริ่มตัวติดตั้ง...",
    retry_update: "ลองตรวจสอบการอัปเดตอีกครั้ง",
    update_failed: "ไม่สามารถอัปเดตให้เสร็จได้",
    about: "เกี่ยวกับ",
    exit: "ออก",
    balloon_startup_on: "เปิดการเริ่มเมื่อเปิดเครื่องแล้ว",
    balloon_startup_off: "ปิดการเริ่มเมื่อเปิดเครื่องแล้ว",
    balloon_rescan_started: "เริ่มสแกนการใช้งานเดือนนี้ทั้งหมดแล้ว",
    balloon_rescan_finished: "สแกนการใช้งานเดือนนี้เสร็จแล้ว",
    balloon_up_to_date: "RunDog เป็นเวอร์ชันล่าสุดแล้ว",
    balloon_available: "มี RunDog v{version} ให้ใช้แล้ว",
    balloon_check_failed: "ตรวจสอบการอัปเดตไม่ได้",
};

#[cfg(test)]
mod tests {
    use super::{primary_langid, MenuText, UiLanguage, ABOUT_PAGE_URL};

    #[test]
    fn component_langid_maps_supported_primary_languages_and_falls_back() {
        assert_eq!(UiLanguage::from_langid(0x0411), UiLanguage::Japanese);
        assert_eq!(UiLanguage::from_langid(0x0409), UiLanguage::English);
        assert_eq!(UiLanguage::from_langid(0x0809), UiLanguage::English);
        assert_eq!(UiLanguage::from_langid(0x0412), UiLanguage::Korean);
        assert_eq!(UiLanguage::from_langid(0x0804), UiLanguage::Chinese);
        assert_eq!(
            UiLanguage::from_langid(0x0404),
            UiLanguage::ChineseTraditional
        );
        assert_eq!(
            UiLanguage::from_langid(0x0C04),
            UiLanguage::ChineseTraditional
        );
        assert_eq!(
            UiLanguage::from_langid(0x1404),
            UiLanguage::ChineseTraditional
        );
        assert_eq!(UiLanguage::from_langid(0x042A), UiLanguage::Vietnamese);
        assert_eq!(UiLanguage::from_langid(0x040C), UiLanguage::French);
        assert_eq!(UiLanguage::from_langid(0x0407), UiLanguage::German);
        assert_eq!(UiLanguage::from_langid(0x0419), UiLanguage::Russian);
        assert_eq!(UiLanguage::from_langid(0x0410), UiLanguage::Italian);
        assert_eq!(UiLanguage::from_langid(0x041E), UiLanguage::Thai);
        assert_eq!(UiLanguage::from_langid(0x040A), UiLanguage::Spanish);
        assert_eq!(UiLanguage::from_langid(0x080A), UiLanguage::Spanish);
        assert_eq!(UiLanguage::from_langid(0), UiLanguage::English);
        assert_eq!(primary_langid(0x0411), 0x11);
    }

    #[test]
    fn component_menu_copy_covers_on_off_and_version_templates() {
        let en = UiLanguage::English.menu();
        assert_eq!(en.startup(true), "Launch at startup: On");
        assert_eq!(en.startup(false), "Launch at startup: Off");
        assert_eq!(en.pinned_flyout(true), "Pin monitor card: On");
        assert_eq!(en.pinned_flyout(false), "Pin monitor card: Off");
        assert_eq!(
            en.with_version(en.install_update, "1.1.17"),
            "Install RunDog v1.1.17"
        );
        assert_eq!(UiLanguage::Japanese.menu().about, "RunDog について");
        assert_eq!(UiLanguage::Japanese.menu().display, "トレイ表示");
        assert_eq!(UiLanguage::Japanese.menu().display_dog, "犬（CPU）");
        assert_eq!(en.display_dog, "Dog (CPU)");
        assert_eq!(
            UiLanguage::Japanese.menu().display_claude_5h,
            "Claude 5時間"
        );
        assert_eq!(UiLanguage::Japanese.menu().display_fable_week, "Fable 週");
        assert_eq!(UiLanguage::Japanese.menu().display_codex_week, "Codex 週");
        assert_eq!(UiLanguage::Thai.menu().exit, "ออก");
        assert_eq!(UiLanguage::Russian.menu().theme, "Тема");
        assert_eq!(UiLanguage::Italian.menu().about, "Informazioni");
        assert_eq!(UiLanguage::Spanish.menu().about, "Acerca de");
        assert_eq!(UiLanguage::Spanish.pages_code(), "es");
        assert_eq!(UiLanguage::ChineseTraditional.menu().about, "關於");
        assert_eq!(UiLanguage::ChineseTraditional.pages_code(), "zh-TW");
    }

    #[test]
    fn component_about_url_uses_pages_lang_except_default_japanese() {
        assert_eq!(UiLanguage::Japanese.about_url(), ABOUT_PAGE_URL);
        assert_eq!(
            UiLanguage::English.about_url(),
            format!("{ABOUT_PAGE_URL}?lang=en")
        );
        assert_eq!(
            UiLanguage::Korean.about_url(),
            format!("{ABOUT_PAGE_URL}?lang=ko")
        );
        assert_eq!(
            UiLanguage::Spanish.about_url(),
            format!("{ABOUT_PAGE_URL}?lang=es")
        );
        assert_eq!(
            UiLanguage::ChineseTraditional.about_url(),
            format!("{ABOUT_PAGE_URL}?lang=zh-TW")
        );
    }

    #[test]
    fn component_every_supported_language_has_required_menu_fields() {
        let langs = [
            UiLanguage::Japanese,
            UiLanguage::English,
            UiLanguage::Korean,
            UiLanguage::Chinese,
            UiLanguage::ChineseTraditional,
            UiLanguage::Vietnamese,
            UiLanguage::French,
            UiLanguage::German,
            UiLanguage::Spanish,
            UiLanguage::Russian,
            UiLanguage::Italian,
            UiLanguage::Thai,
        ];
        for lang in langs {
            let text: MenuText = lang.menu();
            assert!(!text.display.is_empty());
            assert!(!text.display_dog.is_empty());
            assert!(!text.display_claude_5h.is_empty());
            assert!(!text.display_claude_week.is_empty());
            assert!(!text.display_fable_week.is_empty());
            assert!(!text.display_codex_5h.is_empty());
            assert!(!text.display_codex_week.is_empty());
            assert!(!text.theme.is_empty());
            assert!(!text.about.is_empty());
            assert!(!text.exit.is_empty());
            assert!(text.install_update.contains("{version}"));
            assert!(!lang.pages_code().is_empty());
        }
    }
}
