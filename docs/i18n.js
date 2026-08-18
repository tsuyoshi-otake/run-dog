export const LANGS = [
  { id: "ja", label: "日本語" },
  { id: "en", label: "English" },
  { id: "zh", label: "简体中文" },
  { id: "ko", label: "한국어" },
  { id: "vi", label: "Tiếng Việt" },
  { id: "fr", label: "Français" },
  { id: "de", label: "Deutsch" },
  { id: "es", label: "Español" },
];

export const I18N = {
  ja: {
    title: "RunDog",
    description:
      "通知領域で犬を飼ってみませんか？走る速さで Windows の CPU 負荷がわかります。",
    tagline: "通知領域で犬を飼ってみませんか？",
    lead: "犬の走る速さで Windows の CPU 負荷がわかります。Rust で最適化しているので、常駐してもほとんど負荷をかけません。",
    download: "Windows 向けにダウンロード",
    requirement: "Windows 10 / 11（64-bit）",
    viewGithub: "GitHub で見る",
    altTaskbar: "通知領域を走る RunDog",
    altFlyout: "ホバー時の RunDog カード",
    featuresTitle: "特長",
    features: [
      {
        title: "ほぼ無負荷",
        body: "Rust と Win32 だけ。GUI フレームワークも余計なスレッドもありません。リリースは LTO で最適化し、CPU とメモリを最小にしています。",
      },
      {
        title: "ひと目で負荷がわかる",
        body: "CPU が忙しくなるほど犬は速く走り、落ち着いているときはゆっくり歩きます。数字を読む必要はありません。",
      },
      {
        title: "必要なメトリクスをカードで",
        body: "CPU、メモリ、GPU、ストレージ。気になる情報を通知領域からすぐ確認できます。",
      },
      {
        title: "Claude と Codex も見守る",
        body: "サブスクリプションの上限と API 相当の利用料を、CLI を起動せずに表示します。",
      },
    ],
    metricsTitle: "ホバーカード",
    metricsLead: "犬にポインターを重ねるとカードが開きます。数字を探しに行く必要はありません。",
    metrics: [
      "CPU 使用率と System / User / Idle",
      "メモリ使用量",
      "GPU 使用率と専用 / 共有メモリ",
      "ストレージ使用量",
      "Claude Code の 5 時間・7 日上限",
      "Codex CLI の上限と API 相当の利用料",
      "RunDog 自身の CPU とメモリ",
    ],
    usageTitle: "Claude Code と Codex",
    usageBody:
      "サブスクリプションの 5 時間・7 日上限と、API 相当の利用料をカードへ出します。claude や codex を起動せず、必要なログだけを読みます。",
    faqTitle: "よくある質問",
    faq: [
      {
        q: "対応している言語は？",
        a: "この紹介ページは日本語、英語、中国語、韓国語、ベトナム語、フランス語、ドイツ語、スペイン語に対応しています。アプリ本体のカード表記は英語です。",
      },
      {
        q: "RunCat と同じものですか？",
        a: "いいえ。RunDog は Windows 向けに新しく書いた独立したアプリです。RunCat の置き換えではなく、走るペットで負荷を伝えるという発想へのオマージュです。",
      },
      {
        q: "重いですか？",
        a: "いいえ。Rust で Win32 を直接叩いており、GUI フレームワークも余計なスレッドもありません。リリースは LTO で最適化しているので、アイドル時の CPU はマシン全体で 0.1% を下回ることが多く、プライベートメモリは数 MiB です。",
      },
      {
        q: "外部にデータを送りますか？",
        a: "起動時に GitHub Releases へ更新確認を一度行います。Claude や Codex を使っている場合のみ、その認証情報で各社の上限 API を問い合わせます。広告や解析 SDK はありません。",
      },
      {
        q: "SmartScreen の警告が出ますか？",
        a: "SignPath Foundation のコード署名は申請中です。承認までは署名がないため、警告が出ることがあります。インストーラーには SHA-256 の照合があります。公開元の GitHub リポジトリから導入してください。",
      },
      {
        q: "アンインストールできますか？",
        a: "できます。Windows の「アプリ」から RunDog を削除してください。スタートメニューのショートカットと、スタートアップ登録も一緒に外れます。",
      },
      {
        q: "動作環境は？",
        a: "Windows 10 または 11 の 64-bit です。",
      },
    ],
    privacy: "プライバシー",
    privacyTitle: "プライバシー",
    back: "RunDog に戻る",
    privacyBody: [
      "RunDog はアカウントを作りません。CPU・メモリ・GPU・ストレージは Windows の API で端末内だけ読みます。設定はユーザーのレジストリに保存します。",
      "起動時に一度だけ、GitHub Releases へ新しい版があるか確認します。ダウンロードと導入は、メニューから明示したときだけです。",
      "Claude Code や Codex CLI を使っている場合、そのホームディレクトリのログと、すでに端末にある認証情報で各社の上限を問い合わせることがあります。トークンを第三者と共有することはありません。",
      "広告、解析、クラッシュ報告の SDK は入れていません。",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  en: {
    title: "RunDog",
    description:
      "A dog in the notification area. How fast it runs tells you the CPU load on Windows.",
    tagline: "A dog living in the notification area.",
    lead: "The dog tells you Windows CPU usage by how fast it runs. Written in Rust and optimized so it barely uses CPU or memory while it lives in the tray.",
    download: "Download for Windows",
    requirement: "Windows 10 / 11 (64-bit)",
    viewGithub: "View on GitHub",
    altTaskbar: "RunDog running in the notification area",
    altFlyout: "RunDog hover card",
    featuresTitle: "Features",
    features: [
      {
        title: "Almost no overhead",
        body: "Rust on Win32 — no GUI framework, no extra threads. Release builds use LTO so idle CPU and memory stay as small as we can make them.",
      },
      {
        title: "Load at a glance",
        body: "The dog speeds up as your CPU gets busier and slows to a stroll when things are calm. No numbers to read — just watch it run.",
      },
      {
        title: "A compact system card",
        body: "CPU, memory, GPU, and storage — keep an eye on what matters right from the notification area.",
      },
      {
        title: "Claude and Codex, too",
        body: "Subscription rate limits and API-equivalent cost, without launching the CLIs.",
      },
    ],
    metricsTitle: "Hover card",
    metricsLead: "Point at the dog and the card opens. You do not have to go hunting for numbers.",
    metrics: [
      "CPU usage with System / User / Idle",
      "Memory use",
      "GPU use with dedicated / shared memory",
      "Storage use",
      "Claude Code 5-hour and 7-day limits",
      "Codex CLI limits and API-equivalent cost",
      "RunDog's own CPU and memory",
    ],
    usageTitle: "Claude Code and Codex",
    usageBody:
      "The card shows 5-hour and 7-day subscription windows plus API-equivalent cost. RunDog never launches claude or codex — it only reads the logs it needs.",
    faqTitle: "FAQ",
    faq: [
      {
        q: "What languages does it support?",
        a: "This site is in Japanese, English, Chinese, Korean, Vietnamese, French, German, and Spanish. The app card itself is labeled in English.",
      },
      {
        q: "Is this the same as RunCat?",
        a: "No. RunDog is a new Windows app. It is not a replacement for RunCat — it is an homage to the idea of a running pet that shows load.",
      },
      {
        q: "Does it use much CPU or memory?",
        a: "No. It is Rust talking to Win32 directly — no GUI framework, no extra threads. Release builds are LTO-optimized, so idle CPU is often under 0.1% of the machine and private memory is a few MiB.",
      },
      {
        q: "Does it send data off the machine?",
        a: "It checks GitHub Releases once at startup. If you use Claude or Codex, it may query their limit APIs with credentials already on the PC. There is no ads or analytics SDK.",
      },
      {
        q: "Will SmartScreen warn me?",
        a: "Code signing through SignPath Foundation is pending. Until then there is no Authenticode signature, so Windows may warn. The installer is checked with SHA-256. Install from the project's GitHub repository.",
      },
      {
        q: "How do I uninstall?",
        a: "Windows Settings → Apps → RunDog. That also removes Start Menu shortcuts and the optional startup entry.",
      },
      {
        q: "What are the requirements?",
        a: "64-bit Windows 10 or 11.",
      },
    ],
    privacy: "Privacy",
    privacyTitle: "Privacy",
    back: "Back to RunDog",
    privacyBody: [
      "RunDog does not create an account. CPU, memory, GPU, and storage are read locally through Windows APIs. Settings live in the current user's registry.",
      "At startup it checks GitHub Releases once for a newer build. Download and install happen only when you choose them from the menu.",
      "If you use Claude Code or Codex CLI, RunDog may read their local logs and query vendor limit APIs with credentials already on the machine. Tokens are never shared with third parties.",
      "There is no advertising, analytics, or crash-reporting SDK.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  zh: {
    title: "RunDog",
    description: "要不要在通知区域养一只小狗？跑得越快，说明 Windows 的 CPU 越忙。",
    tagline: "要不要在通知区域养一只小狗？",
    lead: "小狗跑得越快，说明 Windows 的 CPU 越忙。用 Rust 优化，常驻时几乎不占用 CPU 和内存。",
    download: "下载 Windows 版",
    requirement: "Windows 10 / 11（64 位）",
    viewGithub: "在 GitHub 上查看",
    altTaskbar: "在通知区域奔跑的 RunDog",
    altFlyout: "悬停时的 RunDog 卡片",
    featuresTitle: "特点",
    features: [
      {
        title: "几乎无负担",
        body: "Rust 直接调用 Win32，没有 GUI 框架，也没有多余线程。发布版用 LTO 优化，把空闲时的 CPU 和内存压到最小。",
      },
      {
        title: "一眼看出负载",
        body: "CPU 越忙，狗跑得越快；空闲时就慢慢走。不用读数字，看它跑就行。",
      },
      {
        title: "卡片里的系统信息",
        body: "CPU、内存、GPU、存储，从通知区域就能确认。",
      },
      {
        title: "也照看 Claude 和 Codex",
        body: "显示订阅限额和相当于 API 的费用，不必启动 CLI。",
      },
    ],
    metricsTitle: "悬停卡片",
    metricsLead: "把指针放到小狗上，卡片就会打开。不必到处找数字。",
    metrics: [
      "CPU 使用率以及 System / User / Idle",
      "内存用量",
      "GPU 使用率及专用 / 共享显存",
      "存储用量",
      "Claude Code 的 5 小时和 7 天限额",
      "Codex CLI 的限额和相当于 API 的费用",
      "RunDog 自身的 CPU 和内存",
    ],
    usageTitle: "Claude Code 与 Codex",
    usageBody:
      "卡片会显示 5 小时、7 天的订阅限额，以及相当于 API 的费用。不会启动 claude 或 codex，只读取必要的日志。",
    faqTitle: "常见问题",
    faq: [
      {
        q: "支持哪些语言？",
        a: "本介绍页支持日语、英语、中文、韩语、越南语、法语、德语和西班牙语。应用卡片上的文字为英语。",
      },
      {
        q: "和 RunCat 是同一个软件吗？",
        a: "不是。RunDog 是为 Windows 新写的独立应用，不是 RunCat 的替代品，只是向“用奔跑的宠物表示负载”这一想法致敬。",
      },
      {
        q: "会很占资源吗？",
        a: "不会。用 Rust 直接调用 Win32，没有 GUI 框架，也没有多余线程。发布版经过 LTO 优化，空闲时整机 CPU 往往低于 0.1%，私有内存只有几 MiB。",
      },
      {
        q: "会把数据发送到外部吗？",
        a: "启动时会向 GitHub Releases 检查一次更新。仅在使用 Claude 或 Codex 时，才会用本机已有的凭据查询各公司的限额 API。没有广告或分析 SDK。",
      },
      {
        q: "SmartScreen 会警告吗？",
        a: "正在申请 SignPath Foundation 的代码签名。获批之前没有 Authenticode 签名，因此可能会出现警告。安装包带有 SHA-256 校验。请从项目的 GitHub 仓库安装。",
      },
      {
        q: "如何卸载？",
        a: "在 Windows「应用」中删除 RunDog。开始菜单快捷方式和可选的开机启动项会一并移除。",
      },
      {
        q: "运行环境是什么？",
        a: "64 位 Windows 10 或 11。",
      },
    ],
    privacy: "隐私",
    privacyTitle: "隐私",
    back: "返回 RunDog",
    privacyBody: [
      "RunDog 不创建账户。CPU、内存、GPU 和存储只通过 Windows API 在本机读取。设置保存在当前用户的注册表中。",
      "启动时只会向 GitHub Releases 检查一次是否有新版本。下载和安装仅在你从菜单明确选择时进行。",
      "如果使用 Claude Code 或 Codex CLI，可能会读取其主目录中的日志，并用本机已有的凭据查询厂商限额 API。令牌不会提供给第三方。",
      "不包含广告、分析或崩溃报告 SDK。",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  ko: {
    title: "RunDog",
    description: "알림 영역에서 강아지를 키워 보시겠어요? 뛰는 속도로 Windows CPU 부하를 알 수 있습니다.",
    tagline: "알림 영역에서 강아지를 키워 보시겠어요?",
    lead: "강아지가 뛰는 속도로 Windows CPU 사용량을 알 수 있습니다. Rust로 최적화해서 상주해도 CPU와 메모리를 거의 쓰지 않습니다.",
    download: "Windows용 다운로드",
    requirement: "Windows 10 / 11 (64비트)",
    viewGithub: "GitHub에서 보기",
    altTaskbar: "알림 영역에서 달리는 RunDog",
    altFlyout: "호버 시 RunDog 카드",
    featuresTitle: "특징",
    features: [
      {
        title: "거의 부하가 없습니다",
        body: "Rust와 Win32만 사용합니다. GUI 프레임워크도 여분 스레드도 없습니다. 릴리스는 LTO로 최적화해 유휴 CPU와 메모리를 최소로 유지합니다.",
      },
      {
        title: "한눈에 부하를 알 수 있습니다",
        body: "CPU가 바빠질수록 강아지는 빨리 달리고, 한가할 때는 천천히 걷습니다. 숫자를 읽을 필요는 없습니다.",
      },
      {
        title: "필요한 지표를 카드로",
        body: "CPU, 메모리, GPU, 저장소. 알림 영역에서 바로 확인할 수 있습니다.",
      },
      {
        title: "Claude와 Codex도 지켜봅니다",
        body: "구독 한도와 API에 해당하는 비용을 CLI를 실행하지 않고 표시합니다.",
      },
    ],
    metricsTitle: "호버 카드",
    metricsLead: "강아지에 포인터를 올리면 카드가 열립니다. 숫자를 찾아다닐 필요가 없습니다.",
    metrics: [
      "CPU 사용률과 System / User / Idle",
      "메모리 사용량",
      "GPU 사용률과 전용 / 공유 메모리",
      "저장소 사용량",
      "Claude Code 5시간·7일 한도",
      "Codex CLI 한도와 API에 해당하는 비용",
      "RunDog 자체의 CPU와 메모리",
    ],
    usageTitle: "Claude Code와 Codex",
    usageBody:
      "구독의 5시간·7일 한도와 API에 해당하는 비용을 카드에 표시합니다. claude나 codex를 실행하지 않고 필요한 로그만 읽습니다.",
    faqTitle: "자주 묻는 질문",
    faq: [
      {
        q: "어떤 언어를 지원하나요?",
        a: "이 소개 페이지는 일본어, 영어, 중국어, 한국어, 베트남어, 프랑스어, 독일어, 스페인어를 지원합니다. 앱 카드의 표기는 영어입니다.",
      },
      {
        q: "RunCat과 같은 앱인가요?",
        a: "아닙니다. RunDog는 Windows용으로 새로 작성한 독립 앱입니다. RunCat을 대체하는 것이 아니라, 달리는 반려동물로 부하를 전한다는 발상에 대한 오마주입니다.",
      },
      {
        q: "무겁지 않나요?",
        a: "아닙니다. Rust로 Win32를 직접 호출하며 GUI 프레임워크와 여분 스레드가 없습니다. 릴리스는 LTO로 최적화되어, 유휴 시 전체 머신 CPU는 대개 0.1% 미만이고 프라이빗 메모리는 수 MiB입니다.",
      },
      {
        q: "데이터를 외부로 보내나요?",
        a: "시작할 때 GitHub Releases에서 업데이트를 한 번 확인합니다. Claude나 Codex를 쓰는 경우에만 이미 있는 자격 증명으로 각 회사의 한도 API를 조회합니다. 광고나 분석 SDK는 없습니다.",
      },
      {
        q: "SmartScreen 경고가 나오나요?",
        a: "SignPath Foundation 코드 서명을 신청 중입니다. 승인 전에는 Authenticode 서명이 없어 경고가 나올 수 있습니다. 설치 파일은 SHA-256으로 검증됩니다. 프로젝트 GitHub 저장소에서 설치하세요.",
      },
      {
        q: "어떻게 제거하나요?",
        a: "Windows 설정 → 앱에서 RunDog를 제거하세요. 시작 메뉴 바로 가기와 선택적 시작 프로그램 등록도 함께 삭제됩니다.",
      },
      {
        q: "동작 환경은?",
        a: "64비트 Windows 10 또는 11입니다.",
      },
    ],
    privacy: "개인정보",
    privacyTitle: "개인정보",
    back: "RunDog로 돌아가기",
    privacyBody: [
      "RunDog는 계정을 만들지 않습니다. CPU·메모리·GPU·저장소는 Windows API로 기기 안에서만 읽습니다. 설정은 현재 사용자 레지스트리에 저장됩니다.",
      "시작할 때 GitHub Releases에서 새 버전이 있는지 한 번만 확인합니다. 다운로드와 설치는 메뉴에서 명시적으로 선택할 때만 진행됩니다.",
      "Claude Code나 Codex CLI를 사용하는 경우 홈 디렉터리 로그와 이미 기기에 있는 자격 증명으로 공급업체 한도 API를 조회할 수 있습니다. 토큰을 제3자와 공유하지 않습니다.",
      "광고, 분석, 충돌 보고 SDK는 포함되어 있지 않습니다.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  vi: {
    title: "RunDog",
    description:
      "Nuôi một chú chó trên khay hệ thống. Tốc độ chạy cho biết CPU của Windows.",
    tagline: "Nuôi một chú chó trên khay hệ thống nhé?",
    lead: "Tốc độ chạy của chú chó cho biết CPU của Windows. Viết bằng Rust và tối ưu để khi chạy nền gần như không tốn CPU hay bộ nhớ.",
    download: "Tải cho Windows",
    requirement: "Windows 10 / 11 (64-bit)",
    viewGithub: "Xem trên GitHub",
    altTaskbar: "RunDog chạy trên khay hệ thống",
    altFlyout: "Thẻ RunDog khi di chuột",
    featuresTitle: "Điểm nổi bật",
    features: [
      {
        title: "Gần như không tốn tài nguyên",
        body: "Rust gọi Win32 trực tiếp — không framework GUI, không luồng thừa. Bản phát hành tối ưu LTO để CPU và bộ nhớ lúc nghỉ nhỏ nhất có thể.",
      },
      {
        title: "Nhìn một cái là biết tải",
        body: "CPU càng bận chó chạy càng nhanh, lúc rảnh thì đi chậm. Không cần đọc số — cứ nhìn nó chạy.",
      },
      {
        title: "Thông số hệ thống trên thẻ",
        body: "CPU, bộ nhớ, GPU, dung lượng lưu trữ — xem ngay từ khay hệ thống.",
      },
      {
        title: "Theo dõi cả Claude và Codex",
        body: "Hạn mức gói đăng ký và chi phí tương đương API, không cần chạy CLI.",
      },
    ],
    metricsTitle: "Thẻ khi di chuột",
    metricsLead: "Đưa con trỏ vào chú chó là thẻ mở ra. Không phải đi tìm số liệu.",
    metrics: [
      "Mức dùng CPU với System / User / Idle",
      "Bộ nhớ",
      "GPU với bộ nhớ dành riêng / dùng chung",
      "Lưu trữ",
      "Hạn mức 5 giờ và 7 ngày của Claude Code",
      "Hạn mức Codex CLI và chi phí tương đương API",
      "CPU và bộ nhớ của chính RunDog",
    ],
    usageTitle: "Claude Code và Codex",
    usageBody:
      "Thẻ hiện hạn mức 5 giờ, 7 ngày của gói đăng ký và chi phí tương đương API. Không khởi chạy claude hay codex — chỉ đọc nhật ký cần thiết.",
    faqTitle: "Câu hỏi thường gặp",
    faq: [
      {
        q: "Hỗ trợ ngôn ngữ nào?",
        a: "Trang giới thiệu có tiếng Nhật, Anh, Trung, Hàn, Việt, Pháp, Đức và Tây Ban Nha. Nhãn trên thẻ ứng dụng là tiếng Anh.",
      },
      {
        q: "Có phải RunCat không?",
        a: "Không. RunDog là ứng dụng Windows viết mới, độc lập. Không phải bản thay RunCat — chỉ tôn vinh ý tưởng thú cưng chạy để báo tải.",
      },
      {
        q: "Có nặng máy không?",
        a: "Không. Rust gọi Win32 trực tiếp, không framework GUI, không luồng thừa. Bản phát hành tối ưu LTO nên lúc nghỉ CPU cả máy thường dưới 0,1% và bộ nhớ riêng chỉ vài MiB.",
      },
      {
        q: "Có gửi dữ liệu ra ngoài không?",
        a: "Khi khởi động sẽ kiểm tra GitHub Releases một lần. Nếu dùng Claude hoặc Codex, có thể gọi API hạn mức bằng thông tin xác thực đã có trên máy. Không có SDK quảng cáo hay phân tích.",
      },
      {
        q: "SmartScreen có cảnh báo không?",
        a: "Đang xin chữ ký SignPath Foundation. Trước khi được duyệt chưa có chữ ký Authenticode nên Windows có thể cảnh báo. Bộ cài được kiểm tra SHA-256. Hãy cài từ kho GitHub của dự án.",
      },
      {
        q: "Gỡ cài đặt thế nào?",
        a: "Windows Cài đặt → Ứng dụng → RunDog. Lối tắt menu Start và mục khởi động tùy chọn cũng bị xóa.",
      },
      {
        q: "Yêu cầu hệ thống?",
        a: "Windows 10 hoặc 11 64-bit.",
      },
    ],
    privacy: "Quyền riêng tư",
    privacyTitle: "Quyền riêng tư",
    back: "Quay lại RunDog",
    privacyBody: [
      "RunDog không tạo tài khoản. CPU, bộ nhớ, GPU và lưu trữ được đọc cục bộ qua API Windows. Cài đặt lưu trong registry của người dùng hiện tại.",
      "Khi khởi động chỉ kiểm tra GitHub Releases một lần xem có bản mới không. Tải và cài chỉ khi bạn chọn rõ từ menu.",
      "Nếu dùng Claude Code hoặc Codex CLI, có thể đọc nhật ký cục bộ và gọi API hạn mức của nhà cung cấp với thông tin xác thực đã có trên máy. Token không chia sẻ với bên thứ ba.",
      "Không có SDK quảng cáo, phân tích hay báo cáo sự cố.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  fr: {
    title: "RunDog",
    description:
      "Un chien dans la zone de notification. Sa vitesse indique la charge CPU de Windows.",
    tagline: "Et si vous adoptiez un chien dans la zone de notification ?",
    lead: "La vitesse du chien indique la charge CPU de Windows. Écrit en Rust et optimisé pour n'utiliser presque ni CPU ni mémoire en résidence.",
    download: "Télécharger pour Windows",
    requirement: "Windows 10 / 11 (64 bits)",
    viewGithub: "Voir sur GitHub",
    altTaskbar: "RunDog dans la zone de notification",
    altFlyout: "Carte RunDog au survol",
    featuresTitle: "Points forts",
    features: [
      {
        title: "Presque aucune charge",
        body: "Rust sur Win32 : pas de framework GUI, pas de threads superflus. Les builds de release utilisent LTO pour garder CPU et mémoire au minimum.",
      },
      {
        title: "La charge d'un coup d'œil",
        body: "Plus le CPU est occupé, plus le chien court vite. Au calme, il se promène. Pas besoin de lire des chiffres.",
      },
      {
        title: "Une carte système compacte",
        body: "CPU, mémoire, GPU et stockage — le nécessaire, depuis la zone de notification.",
      },
      {
        title: "Claude et Codex aussi",
        body: "Plafonds d'abonnement et coût équivalent API, sans lancer les CLI.",
      },
    ],
    metricsTitle: "Carte au survol",
    metricsLead: "Pointez le chien : la carte s'ouvre. Inutile d'aller chercher les chiffres.",
    metrics: [
      "Utilisation CPU avec System / User / Idle",
      "Mémoire",
      "GPU avec mémoire dédiée / partagée",
      "Stockage",
      "Plafonds 5 h et 7 j de Claude Code",
      "Plafonds Codex CLI et coût équivalent API",
      "CPU et mémoire de RunDog lui-même",
    ],
    usageTitle: "Claude Code et Codex",
    usageBody:
      "La carte affiche les fenêtres d'abonnement 5 h et 7 j, plus le coût équivalent API. RunDog ne lance ni claude ni codex : il ne lit que les journaux nécessaires.",
    faqTitle: "FAQ",
    faq: [
      {
        q: "Quelles langues sont prises en charge ?",
        a: "Ce site existe en japonais, anglais, chinois, coréen, vietnamien, français, allemand et espagnol. Les libellés de la carte de l'application sont en anglais.",
      },
      {
        q: "Est-ce le même logiciel que RunCat ?",
        a: "Non. RunDog est une application Windows écrite à neuf. Ce n'est pas un remplacement de RunCat, mais un hommage à l'idée d'un animal qui court pour indiquer la charge.",
      },
      {
        q: "Est-ce lourd ?",
        a: "Non. Rust parle à Win32 directement, sans framework GUI ni threads superflus. Les builds LTO font que le CPU idle est souvent sous 0,1 % de la machine, et la mémoire privée quelques MiB.",
      },
      {
        q: "Envoie-t-il des données à l'extérieur ?",
        a: "Au démarrage, il interroge GitHub Releases une fois. Si vous utilisez Claude ou Codex, il peut interroger leurs API de plafond avec des identifiants déjà présents. Pas de SDK pub ou d'analyse.",
      },
      {
        q: "SmartScreen va-t-il m'avertir ?",
        a: "La signature SignPath Foundation est en cours de demande. En attendant, il n'y a pas de signature Authenticode, donc Windows peut avertir. L'installateur est vérifié par SHA-256. Installez depuis le dépôt GitHub du projet.",
      },
      {
        q: "Comment désinstaller ?",
        a: "Paramètres Windows → Applications → RunDog. Les raccourcis du menu Démarrer et l'entrée de démarrage facultative sont aussi retirés.",
      },
      {
        q: "Quelle configuration ?",
        a: "Windows 10 ou 11 64 bits.",
      },
    ],
    privacy: "Confidentialité",
    privacyTitle: "Confidentialité",
    back: "Retour à RunDog",
    privacyBody: [
      "RunDog ne crée pas de compte. CPU, mémoire, GPU et stockage sont lus localement via les API Windows. Les réglages sont dans le registre de l'utilisateur.",
      "Au démarrage, il vérifie une fois GitHub Releases. Le téléchargement et l'installation n'ont lieu que si vous les choisissez dans le menu.",
      "Si vous utilisez Claude Code ou Codex CLI, RunDog peut lire leurs journaux locaux et interroger les API de plafond avec des identifiants déjà sur la machine. Les jetons ne sont pas partagés avec des tiers.",
      "Aucun SDK de publicité, d'analyse ou de rapport de plantage.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  de: {
    title: "RunDog",
    description:
      "Ein Hund im Infobereich. Wie schnell er läuft, zeigt die CPU-Last von Windows.",
    tagline: "Möchten Sie einen Hund im Infobereich halten?",
    lead: "Wie schnell der Hund läuft, zeigt die CPU-Last von Windows. In Rust optimiert, damit es im Infobereich kaum CPU oder Speicher braucht.",
    download: "Für Windows herunterladen",
    requirement: "Windows 10 / 11 (64-Bit)",
    viewGithub: "Auf GitHub ansehen",
    altTaskbar: "RunDog im Infobereich",
    altFlyout: "RunDog-Karte beim Zeigen",
    featuresTitle: "Merkmale",
    features: [
      {
        title: "Kaum Overhead",
        body: "Rust auf Win32 — kein GUI-Framework, keine Extra-Threads. Release-Builds nutzen LTO, damit Idle-CPU und Speicher so klein wie möglich bleiben.",
      },
      {
        title: "Last auf einen Blick",
        body: "Je beschäftigter die CPU, desto schneller läuft der Hund. In Ruhe geht er spazieren. Keine Zahlen lesen — einfach zusehen.",
      },
      {
        title: "Kompakte Systemkarte",
        body: "CPU, Speicher, GPU und Datenträger — das Wichtige direkt aus dem Infobereich.",
      },
      {
        title: "Auch Claude und Codex",
        body: "Abo-Limits und API-äquivalente Kosten, ohne die CLIs zu starten.",
      },
    ],
    metricsTitle: "Karte beim Zeigen",
    metricsLead: "Zeigen Sie auf den Hund, und die Karte öffnet sich. Zahlen muss man nicht suchen.",
    metrics: [
      "CPU-Auslastung mit System / User / Idle",
      "Speicher",
      "GPU mit dediziertem / gemeinsamem Speicher",
      "Datenträger",
      "5-Stunden- und 7-Tage-Limits von Claude Code",
      "Codex-CLI-Limits und API-äquivalente Kosten",
      "CPU und Arbeitsspeicher von RunDog selbst",
    ],
    usageTitle: "Claude Code und Codex",
    usageBody:
      "Die Karte zeigt 5-Stunden- und 7-Tage-Abo-Fenster sowie API-äquivalente Kosten. RunDog startet weder claude noch codex — es liest nur die nötigen Protokolle.",
    faqTitle: "Häufige Fragen",
    faq: [
      {
        q: "Welche Sprachen werden unterstützt?",
        a: "Diese Seite gibt es auf Japanisch, Englisch, Chinesisch, Koreanisch, Vietnamesisch, Französisch, Deutsch und Spanisch. Die Kartenbeschriftung der App ist Englisch.",
      },
      {
        q: "Ist das dasselbe wie RunCat?",
        a: "Nein. RunDog ist eine neue, eigenständige Windows-App. Kein Ersatz für RunCat, sondern eine Hommage an das laufende Haustier als Lastanzeige.",
      },
      {
        q: "Ist es schwer?",
        a: "Nein. Rust spricht direkt mit Win32 — kein GUI-Framework, keine Extra-Threads. LTO-optimierte Releases liegen im Idle oft unter 0,1 % der Maschinen-CPU, der private Speicher bei wenigen MiB.",
      },
      {
        q: "Werden Daten nach außen gesendet?",
        a: "Beim Start prüft es einmal GitHub Releases. Wenn Sie Claude oder Codex nutzen, kann es deren Limit-APIs mit bereits vorhandenen Anmeldedaten abfragen. Kein Werbe- oder Analyse-SDK.",
      },
      {
        q: "Warnt SmartScreen?",
        a: "Die SignPath-Foundation-Signatur ist beantragt. Bis zur Freigabe gibt es keine Authenticode-Signatur, daher kann Windows warnen. Der Installer wird per SHA-256 geprüft. Installieren Sie aus dem GitHub-Repository des Projekts.",
      },
      {
        q: "Wie deinstalliere ich?",
        a: "Windows-Einstellungen → Apps → RunDog. Startmenü-Verknüpfungen und der optionale Autostart-Eintrag werden mit entfernt.",
      },
      {
        q: "Welche Voraussetzungen?",
        a: "64-Bit-Windows 10 oder 11.",
      },
    ],
    privacy: "Datenschutz",
    privacyTitle: "Datenschutz",
    back: "Zurück zu RunDog",
    privacyBody: [
      "RunDog legt kein Konto an. CPU, Speicher, GPU und Datenträger werden lokal über Windows-APIs gelesen. Einstellungen liegen in der Registry des aktuellen Benutzers.",
      "Beim Start prüft es einmal GitHub Releases auf eine neuere Version. Download und Installation nur, wenn Sie sie im Menü wählen.",
      "Wenn Sie Claude Code oder Codex CLI nutzen, kann RunDog lokale Protokolle lesen und Limit-APIs der Anbieter mit bereits vorhandenen Anmeldedaten abfragen. Tokens werden nicht an Dritte weitergegeben.",
      "Kein SDK für Werbung, Analyse oder Absturzberichte.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  es: {
    title: "RunDog",
    description:
      "Un perro en el área de notificación. Su velocidad indica la carga de CPU de Windows.",
    tagline: "¿Y si crías un perro en el área de notificación?",
    lead: "La velocidad del perro indica la carga de CPU de Windows. Escrito en Rust y optimizado para usar casi nada de CPU ni memoria mientras vive en la bandeja.",
    download: "Descargar para Windows",
    requirement: "Windows 10 / 11 (64 bits)",
    viewGithub: "Ver en GitHub",
    altTaskbar: "RunDog en el área de notificación",
    altFlyout: "Tarjeta de RunDog al pasar el puntero",
    featuresTitle: "Características",
    features: [
      {
        title: "Casi sin carga",
        body: "Rust sobre Win32: sin framework GUI ni hilos extra. Las compilaciones de release usan LTO para dejar la CPU y la memoria en reposo lo más bajas posible.",
      },
      {
        title: "La carga de un vistazo",
        body: "Cuanto más ocupada está la CPU, más rápido corre el perro. En calma, pasea. No hace falta leer números.",
      },
      {
        title: "Una tarjeta de sistema compacta",
        body: "CPU, memoria, GPU y almacenamiento: lo importante, desde el área de notificación.",
      },
      {
        title: "También Claude y Codex",
        body: "Límites de la suscripción y coste equivalente a la API, sin lanzar las CLI.",
      },
    ],
    metricsTitle: "Tarjeta al pasar el puntero",
    metricsLead: "Apunta al perro y se abre la tarjeta. No hay que ir a buscar los números.",
    metrics: [
      "Uso de CPU con System / User / Idle",
      "Memoria",
      "GPU con memoria dedicada / compartida",
      "Almacenamiento",
      "Límites de 5 h y 7 d de Claude Code",
      "Límites de Codex CLI y coste equivalente a la API",
      "CPU y memoria del propio RunDog",
    ],
    usageTitle: "Claude Code y Codex",
    usageBody:
      "La tarjeta muestra las ventanas de suscripción de 5 h y 7 d, más el coste equivalente a la API. RunDog no lanza claude ni codex: solo lee los registros necesarios.",
    faqTitle: "Preguntas frecuentes",
    faq: [
      {
        q: "¿Qué idiomas admite?",
        a: "Este sitio está en japonés, inglés, chino, coreano, vietnamita, francés, alemán y español. Las etiquetas de la tarjeta de la aplicación están en inglés.",
      },
      {
        q: "¿Es lo mismo que RunCat?",
        a: "No. RunDog es una aplicación de Windows escrita de nuevo. No sustituye a RunCat: es un homenaje a la idea de una mascota que corre para mostrar la carga.",
      },
      {
        q: "¿Es pesado?",
        a: "No. Habla con Win32 en Rust, sin framework GUI ni hilos extra. Con LTO, en reposo la CPU de la máquina suele estar por debajo del 0,1 % y la memoria privada en unos pocos MiB.",
      },
      {
        q: "¿Envía datos al exterior?",
        a: "Al arrancar consulta GitHub Releases una vez. Si usas Claude o Codex, puede consultar sus API de límites con credenciales ya presentes. No hay SDK de anuncios ni de analítica.",
      },
      {
        q: "¿Avisará SmartScreen?",
        a: "La firma de SignPath Foundation está en trámite. Hasta entonces no hay firma Authenticode, así que Windows puede avisar. El instalador se comprueba con SHA-256. Instálalo desde el repositorio GitHub del proyecto.",
      },
      {
        q: "¿Cómo se desinstala?",
        a: "Configuración de Windows → Aplicaciones → RunDog. También se quitan los accesos del menú Inicio y el inicio opcional.",
      },
      {
        q: "¿Qué se necesita?",
        a: "Windows 10 u 11 de 64 bits.",
      },
    ],
    privacy: "Privacidad",
    privacyTitle: "Privacidad",
    back: "Volver a RunDog",
    privacyBody: [
      "RunDog no crea una cuenta. CPU, memoria, GPU y almacenamiento se leen en el equipo con las API de Windows. Los ajustes viven en el registro del usuario actual.",
      "Al arrancar comprueba GitHub Releases una sola vez. La descarga e instalación solo ocurren si las eliges en el menú.",
      "Si usas Claude Code o Codex CLI, puede leer sus registros locales y consultar las API de límites con credenciales ya en el equipo. Los tokens no se comparten con terceros.",
      "No incluye SDK de publicidad, analítica ni informes de fallos.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
};
