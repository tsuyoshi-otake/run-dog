export const LANGS = [
  { id: "ja", label: "日本語" },
  { id: "en", label: "English" },
  { id: "zh", label: "简体中文" },
  { id: "zh-TW", label: "繁體中文" },
  { id: "ko", label: "한국어" },
  { id: "vi", label: "Tiếng Việt" },
  { id: "fr", label: "Français" },
  { id: "de", label: "Deutsch" },
  { id: "es", label: "Español" },
  { id: "ru", label: "Русский" },
  { id: "it", label: "Italiano" },
  { id: "th", label: "ไทย" },
];

export const I18N = {
  ja: {
    title: "RunDog",
    description:
      "通知領域で犬を飼ってみませんか？走る速さで Windows の CPU 負荷がわかります。",
    tagline: "通知領域で犬を飼ってみませんか？",
    lead: "犬の走る速さで Windows の CPU 負荷がわかります。右クリックのトレイ表示から、CPU / メモリ / GPU や Claude / Codex の 5 時間・週次、Fable 週次を数値アイコンにもできます。Rust で最適化しているので、常駐してもほとんど負荷をかけません。",
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
        body: "CPU が忙しくなるほど犬は速く走り、落ち着いているときはゆっくり歩きます。右クリックのトレイ表示から、数字を通知領域に出すこともできます。",
      },
      {
        title: "トレイを数値にもできる",
        body: "トレイ表示で犬（CPU）、CPU、メモリ、GPU、Claude 5時間、Claude 週、Fable 週、Codex 5時間、Codex 週を切り替えます。古い・無いリミットは -- で、0% は作りません。設定は残ります。",
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
    metricsLead: "犬や数値アイコンにポインターを重ねるとカードが開きます。トレイの数字は右クリックのトレイ表示からも出せます。",
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
        a: "紹介ページは日本語、英語、簡体字中国語、繁体字中国語、韓国語、ベトナム語、フランス語、ドイツ語、スペイン語、ロシア語、イタリア語、タイ語に対応しています。右クリックメニューは Windows の表示言語に合わせます。ホバーカードの表記は英語です。",
      },
      {
        q: "トレイに数字を出せますか？",
        a: "はい。右クリックのトレイ表示から、犬のアニメーションと、CPU / メモリ / GPU の整数パーセント、Claude 5時間・Claude 週、Fable 週、Codex 5時間・Codex 週を切り替えられます。古い・無いリミットは -- です。設定は残ります。",
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
        a: "起動時に GitHub Releases を一度確認します。Claude / Codex があるときだけ各社の上限 API を呼びます。RunDog 自身のサーバーへは送りません。生プロンプトは送りません。広告や解析 SDK はありません。",
      },
      {
        q: "SmartScreen の警告が出ますか？",
        a: "SignPath Foundation のコード署名は申請中です。承認までは署名がないため、警告が出ることがあります。インストーラーには SHA-256 の照合があります。公開元の GitHub リポジトリから導入してください。",
      },
      {
        q: "アンインストールできますか？",
        a: "できます。Windows の「アプリ」から RunDog を削除してください。プログラム、ショートカット、スタートアップ、設定、利用状況、更新キャッシュは消えます。Claude と Codex のログ・認証情報・ホームは残します。",
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
      "RunDog はアカウントを作りません。CPU・メモリ・GPU・ストレージは Windows の API で端末内だけ読みます。設定は HKCU\\Software\\SystemExe\\RunDog に保存します。",
      "読むファイルは Claude の projects JSONL と .credentials.json、Codex の sessions JSONL と auth.json、および RunDog 自身の %LOCALAPPDATA%\\RunDog\\usage です。JSONL の生プロンプトや応答を上限取得で送りません。",
      "送信先は GitHub（api.github.com / github.com、起動時の更新確認と明示した導入）、Anthropic（api.anthropic.com の usage、必要時のみ platform.claude.com または console.anthropic.com の token refresh）、OpenAI（chatgpt.com の wham/usage）です。RunDog 自身のサーバーへは送りません。",
      "Claude の上限問い合わせは約 5 分ごと、Codex は約 60 秒ごとです。期限切れや認証失敗のとき Claude の refresh が走り、.credentials.json を ReplaceFileW で更新することがあります。Codex の auth.json は読み取りのみです。",
      "ベンダー上限の自動問い合わせは、Claude / Codex が端末にあるとき従来どおり On です。新しい「Automatic vendor usage-limit queries」設定はメニューと UX を壊すので見送りました。止めたい場合は認証情報か CLI を外してください。",
      "広告、解析、クラッシュ報告の SDK は入れていません。",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  en: {
    title: "RunDog",
    description:
      "A dog in the notification area. How fast it runs tells you the CPU load on Windows.",
    tagline: "A dog living in the notification area.",
    lead: "The dog tells you Windows CPU usage by how fast it runs. From the right-click Tray icon menu you can also put CPU, memory, GPU, Claude or Codex 5-hour and weekly limits, or Fable week in the notification-area icon. Written in Rust and optimized so it barely uses CPU or memory while it lives in the tray.",
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
        body: "The dog speeds up as your CPU gets busier and slows to a stroll when things are calm. Or put numbers in the tray from the right-click Tray icon menu.",
      },
      {
        title: "Numbers in the tray",
        body: "Tray icon switches among Dog (CPU), CPU, Memory, GPU, Claude 5h, Claude week, Fable week, Codex 5h, and Codex week. Stale or missing limits show --, never a fabricated 0%. The setting persists.",
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
    metricsLead: "Point at the dog or a number icon and the card opens. You can also put those percents in the tray from the right-click Tray icon menu.",
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
        a: "This site is in Japanese, English, Simplified Chinese, Traditional Chinese, Korean, Vietnamese, French, German, Spanish, Russian, Italian, and Thai. The tray menu follows the Windows display language. The hover card remains in English.",
      },
      {
        q: "Can the tray show numbers?",
        a: "Yes. Right-click Tray icon to switch among the running dog and integer percents for CPU, Memory, GPU, Claude 5h, Claude week, Fable week, Codex 5h, and Codex week. Stale or missing limits show --. The choice is saved.",
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
        a: "It checks GitHub Releases once at startup. If Claude or Codex is present it queries those vendors' limit APIs. Nothing goes to a RunDog-owned server. Raw prompts are not sent. There is no ads or analytics SDK.",
      },
      {
        q: "Will SmartScreen warn me?",
        a: "Code signing through SignPath Foundation is pending. Until then there is no Authenticode signature, so Windows may warn. The installer is checked with SHA-256. Install from the project's GitHub repository.",
      },
      {
        q: "How do I uninstall?",
        a: "Windows Settings → Apps → RunDog. That removes the program, shortcuts, startup entry, settings, usage store, and update cache. Claude and Codex logs, credentials, and homes stay.",
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
      "RunDog does not create an account. CPU, memory, GPU, and storage are read locally through Windows APIs. Settings live in HKCU\\Software\\SystemExe\\RunDog.",
      "It may read Claude projects JSONL and .credentials.json, Codex sessions JSONL and auth.json, plus RunDog's own %LOCALAPPDATA%\\RunDog\\usage store. Limit fetch does not send raw prompts or responses.",
      "Destinations are GitHub (api.github.com / github.com — startup update check and explicit Install), Anthropic (api.anthropic.com usage; token refresh on platform.claude.com or console.anthropic.com when needed), and OpenAI (chatgpt.com wham/usage). There is no RunDog-owned server.",
      "Claude limit queries run about every 5 minutes; Codex about every 60 seconds. An expired or unauthorized Claude token may refresh and rewrite .credentials.json via ReplaceFileW. Codex auth.json is read-only.",
      "Automatic vendor limit queries stay On when those CLIs are present. A new Automatic vendor usage-limit queries setting is deferred so the tray UX is not broken. Remove credentials or the CLI to stop queries.",
      "There is no advertising, analytics, or crash-reporting SDK.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  zh: {
    title: "RunDog",
    description: "要不要在通知区域养一只小狗？跑得越快，说明 Windows 的 CPU 越忙。",
    tagline: "要不要在通知区域养一只小狗？",
    lead: "小狗跑得越快，说明 Windows 的 CPU 越忙。也可以在右键「托盘图标」里，把通知区改成 CPU / 内存 / GPU 百分比，或 Claude / Codex 的 5 小时与每周限额、Fable 每周限额。用 Rust 优化，常驻时几乎不占用 CPU 和内存。",
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
        body: "CPU 越忙，狗跑得越快；空闲时就慢慢走。也可以从右键「托盘图标」把数字放到通知区。",
      },
      {
        title: "托盘也可以显示数字",
        body: "「托盘图标」可在狗（CPU）、CPU、内存、GPU、Claude 5 小时、Claude 每周、Fable 每周、Codex 5 小时、Codex 每周之间切换。过期或缺失的限额显示 --，不会编造 0%。选择会保留。",
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
    metricsLead: "把指针放到小狗或数字图标上，卡片就会打开。也可以从右键「托盘图标」把这些百分比放到托盘里。",
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
        a: "本介绍页支持日语、英语、简体中文、繁体中文、韩语、越南语、法语、德语、西班牙语、俄语、意大利语和泰语。右键菜单跟随 Windows 显示语言。悬停卡片上的文字为英语。",
      },
      {
        q: "托盘能显示数字吗？",
        a: "可以。右键「托盘图标」可在奔跑的小狗和 CPU、内存、GPU、Claude 5 小时、Claude 每周、Fable 每周、Codex 5 小时、Codex 每周的整数百分比之间切换。过期或缺失的限额显示 --。选择会保存。",
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
        a: "在 Windows「应用」中删除 RunDog。程序、快捷方式、开机启动、设置、用量数据和更新缓存会删除。Claude 与 Codex 的日志、凭据和主目录会保留。",
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
      "RunDog 不创建账户。CPU、内存、GPU 和存储只通过 Windows API 在本机读取。设置保存在 HKCU\\Software\\SystemExe\\RunDog。",
      "可能读取 Claude 的 projects JSONL 与 .credentials.json、Codex 的 sessions JSONL 与 auth.json，以及 RunDog 自己的 LOCALAPPDATA 存储。限额查询不会发送原始提示或回复。",
      "发送目标是 GitHub、Anthropic（api.anthropic.com，必要时 token refresh）和 chatgpt.com。没有 RunDog 自己的服务器。Claude 约每 5 分钟、Codex 约每 60 秒查询一次；过期时可能改写 .credentials.json。",
      "有 CLI 时自动查询限额保持开启。未新增会破坏菜单的开关。不包含广告、分析或崩溃报告 SDK。",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  "zh-TW": {
    title: "RunDog",
    description: "要不要在通知區域養一隻小狗？跑得越快，代表 Windows 的 CPU 越忙。",
    tagline: "要不要在通知區域養一隻小狗？",
    lead: "小狗跑得越快，代表 Windows 的 CPU 越忙。也可以在右鍵「工作列圖示」裡，把通知區改成 CPU / 記憶體 / GPU 百分比，或 Claude / Codex 的 5 小時與每週限額、Fable 每週限額。用 Rust 最佳化，常駐時幾乎不佔用 CPU 和記憶體。",
    download: "下載 Windows 版",
    requirement: "Windows 10 / 11（64 位元）",
    viewGithub: "在 GitHub 上查看",
    altTaskbar: "在通知區域奔跑的 RunDog",
    altFlyout: "懸停時的 RunDog 卡片",
    featuresTitle: "特點",
    features: [
      {
        title: "幾乎無負擔",
        body: "Rust 直接呼叫 Win32，沒有 GUI 框架，也沒有多餘執行緒。發行版用 LTO 最佳化，把閒置時的 CPU 和記憶體壓到最小。",
      },
      {
        title: "一眼看出負載",
        body: "CPU 越忙，狗跑得越快；空閒時就慢慢走。也可以從右鍵「工作列圖示」把數字放到通知區。",
      },
      {
        title: "工作列也可以顯示數字",
        body: "「工作列圖示」可在狗（CPU）、CPU、記憶體、GPU、Claude 5 小時、Claude 每週、Fable 每週、Codex 5 小時、Codex 每週之間切換。過期或缺失的限額顯示 --，不會編造 0%。選擇會保留。",
      },
      {
        title: "卡片裡的系統資訊",
        body: "CPU、記憶體、GPU、儲存，從通知區域就能確認。",
      },
      {
        title: "也照看 Claude 和 Codex",
        body: "顯示訂閱限額和相當於 API 的費用，不必啟動 CLI。",
      },
    ],
    metricsTitle: "懸停卡片",
    metricsLead: "把指標放到小狗或數字圖示上，卡片就會打開。也可以從右鍵「工作列圖示」把這些百分比放到工作列。",
    metrics: [
      "CPU 使用率以及 System / User / Idle",
      "記憶體用量",
      "GPU 使用率及專用 / 共用顯示記憶體",
      "儲存用量",
      "Claude Code 的 5 小時和 7 天限額",
      "Codex CLI 的限額和相當於 API 的費用",
      "RunDog 自身的 CPU 和記憶體",
    ],
    usageTitle: "Claude Code 與 Codex",
    usageBody:
      "卡片會顯示 5 小時、7 天的訂閱限額，以及相當於 API 的費用。不會啟動 claude 或 codex，只讀取必要的紀錄。",
    faqTitle: "常見問題",
    faq: [
      {
        q: "支援哪些語言？",
        a: "本介紹頁支援日文、英文、簡體中文、繁體中文、韓文、越南文、法文、德文、西班牙文、俄文、義大利文和泰文。右鍵選單會跟隨 Windows 顯示語言。懸停卡片上的文字為英文。",
      },
      {
        q: "工作列能顯示數字嗎？",
        a: "可以。右鍵「工作列圖示」可在奔跑的小狗和 CPU、記憶體、GPU、Claude 5 小時、Claude 每週、Fable 每週、Codex 5 小時、Codex 每週的整數百分比之間切換。過期或缺失的限額顯示 --。選擇會儲存。",
      },
      {
        q: "和 RunCat 是同一個軟體嗎？",
        a: "不是。RunDog 是為 Windows 新寫的獨立應用，不是 RunCat 的替代品，只是向「用奔跑的寵物表示負載」這個想法致敬。",
      },
      {
        q: "會很佔資源嗎？",
        a: "不會。用 Rust 直接呼叫 Win32，沒有 GUI 框架，也沒有多餘執行緒。發行版經過 LTO 最佳化，閒置時整機 CPU 往往低於 0.1%，私用記憶體只有幾 MiB。",
      },
      {
        q: "會把資料傳送到外部嗎？",
        a: "啟動時會向 GitHub Releases 檢查一次更新。僅在使用 Claude 或 Codex 時，才會用本機既有的認證資料查詢各公司的限額 API。沒有廣告或分析 SDK。",
      },
      {
        q: "SmartScreen 會警告嗎？",
        a: "正在申請 SignPath Foundation 的程式碼簽署。獲准之前沒有 Authenticode 簽章，因此可能會出現警告。安裝套件帶有 SHA-256 核對。請從專案的 GitHub 存放庫安裝。",
      },
      {
        q: "如何解除安裝？",
        a: "在 Windows「應用程式」中刪除 RunDog。程式、捷徑、開機啟動、設定、用量資料和更新快取會刪除。Claude 與 Codex 的紀錄、憑證和主目錄會保留。",
      },
      {
        q: "執行環境是什麼？",
        a: "64 位元 Windows 10 或 11。",
      },
    ],
    privacy: "隱私權",
    privacyTitle: "隱私權",
    back: "返回 RunDog",
    privacyBody: [
      "RunDog 不建立帳戶。CPU、記憶體、GPU 和儲存只透過 Windows API 在本機讀取。設定保存在 HKCU\\Software\\SystemExe\\RunDog。",
      "可能讀取 Claude 的 projects JSONL 與 .credentials.json、Codex 的 sessions JSONL 與 auth.json，以及 RunDog 自己的 LOCALAPPDATA。限額查詢不會傳送原始提示或回覆。",
      "傳送目標是 GitHub、Anthropic 與 chatgpt.com。沒有 RunDog 自己的伺服器。Claude 約每 5 分鐘、Codex 約每 60 秒查一次；過期時可能改寫 .credentials.json。",
      "有 CLI 時自動查詢維持開啟。未新增會破壞選單的開關。不含廣告、分析或當機回報 SDK。",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  ko: {
    title: "RunDog",
    description: "알림 영역에서 강아지를 키워 보시겠어요? 뛰는 속도로 Windows CPU 부하를 알 수 있습니다.",
    tagline: "알림 영역에서 강아지를 키워 보시겠어요?",
    lead: "강아지가 뛰는 속도로 Windows CPU 사용량을 알 수 있습니다. 오른쪽 클릭 트레이 아이콘에서 CPU / 메모리 / GPU 퍼센트나 Claude / Codex 5시간·주간, Fable 주간 한도를 알림 영역 아이콘으로 바꿀 수도 있습니다. Rust로 최적화해서 상주해도 CPU와 메모리를 거의 쓰지 않습니다.",
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
        body: "CPU가 바빠질수록 강아지는 빨리 달리고, 한가할 때는 천천히 걷습니다. 오른쪽 클릭 트레이 아이콘에서 숫자를 알림 영역에 둘 수도 있습니다.",
      },
      {
        title: "트레이에 숫자도 표시",
        body: "트레이 아이콘에서 개 (CPU), CPU, 메모리, GPU, Claude 5시간, Claude 주간, Fable 주간, Codex 5시간, Codex 주간을 전환합니다. 오래되거나 없는 한도는 --이며, 0%를 만들지 않습니다. 설정은 유지됩니다.",
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
    metricsLead: "강아지나 숫자 아이콘에 포인터를 올리면 카드가 열립니다. 오른쪽 클릭 트레이 아이콘에서 그 퍼센트를 트레이에 둘 수도 있습니다.",
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
        a: "이 소개 페이지는 일본어, 영어, 중국어, 한국어, 베트남어, 프랑스어, 독일어, 스페인어, 러시아어, 이탈리아어, 태국어를 지원합니다. 오른쪽 클릭 메뉴는 Windows 표시 언어를 따릅니다. 호버 카드의 표기는 영어입니다.",
      },
      {
        q: "트레이에 숫자를 표시할 수 있나요?",
        a: "네. 오른쪽 클릭 트레이 아이콘에서 달리는 개와 CPU, 메모리, GPU, Claude 5시간, Claude 주간, Fable 주간, Codex 5시간, Codex 주간의 정수 퍼센트를 전환합니다. 오래되거나 없는 한도는 --입니다. 선택은 저장됩니다.",
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
        a: "Windows 설정 → 앱에서 RunDog를 제거하세요. 프로그램, 바로 가기, 시작 등록, 설정, 사용량, 업데이트 캐시는 삭제됩니다. Claude와 Codex의 로그, 자격 증명, 홈은 남습니다.",
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
      "RunDog는 계정을 만들지 않습니다. CPU·메모리·GPU·저장소는 Windows API로 기기 안에서만 읽습니다. 설정은 HKCU\\Software\\SystemExe\\RunDog에 있습니다.",
      "Claude projects JSONL과 .credentials.json, Codex sessions JSONL과 auth.json, RunDog LOCALAPPDATA 저장소를 읽을 수 있습니다. 한도 조회에 raw prompt/response는 보내지 않습니다.",
      "전송처는 GitHub, Anthropic, chatgpt.com입니다. RunDog 자체 서버는 없습니다. Claude는 약 5분, Codex는 약 60초마다 조회하며, 만료 시 .credentials.json을 다시 쓸 수 있습니다.",
      "CLI가 있으면 자동 한도 조회는 그대로 On입니다. 메뉴를 깨는 새 설정은 넣지 않았습니다. 광고·분석·충돌 SDK는 없습니다.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  vi: {
    title: "RunDog",
    description:
      "Nuôi một chú chó trên khay hệ thống. Tốc độ chạy cho biết CPU của Windows.",
    tagline: "Nuôi một chú chó trên khay hệ thống nhé?",
    lead: "Tốc độ chạy của chú chó cho biết CPU của Windows. Menu chuột phải Biểu tượng khay cũng có thể đưa phần trăm CPU / bộ nhớ / GPU, hạn mức 5 giờ và tuần của Claude / Codex, và hạn mức tuần Fable lên biểu tượng khay. Viết bằng Rust và tối ưu để khi chạy nền gần như không tốn CPU hay bộ nhớ.",
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
        body: "CPU càng bận chó chạy càng nhanh, lúc rảnh thì đi chậm. Cũng có thể đưa số lên khay từ menu chuột phải Biểu tượng khay.",
      },
      {
        title: "Số trên khay",
        body: "Biểu tượng khay chuyển giữa Chó (CPU), CPU, Bộ nhớ, GPU, Claude 5 giờ, Claude tuần, Fable tuần, Codex 5 giờ và Codex tuần. Hạn mức cũ hoặc thiếu hiện --, không bịa 0%. Lựa chọn được lưu.",
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
    metricsLead: "Đưa con trỏ vào chú chó hoặc biểu tượng số là thẻ mở ra. Cũng có thể đưa các phần trăm đó lên khay từ menu chuột phải Biểu tượng khay.",
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
        a: "Trang giới thiệu có tiếng Nhật, Anh, Trung, Hàn, Việt, Pháp, Đức, Tây Ban Nha, Nga, Ý và Thái. Menu chuột phải theo ngôn ngữ hiển thị của Windows. Nhãn trên thẻ khi di chuột là tiếng Anh.",
      },
      {
        q: "Khay có hiện số được không?",
        a: "Có. Chuột phải Biểu tượng khay để chuyển giữa chó chạy và phần trăm nguyên của CPU, Bộ nhớ, GPU, Claude 5 giờ, Claude tuần, Fable tuần, Codex 5 giờ và Codex tuần. Hạn mức cũ hoặc thiếu hiện --. Lựa chọn được lưu.",
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
        a: "Windows Cài đặt → Ứng dụng → RunDog. Chương trình, lối tắt, khởi động, cài đặt, dữ liệu usage và cache cập nhật bị xóa. Nhật ký, thông tin đăng nhập và thư mục nhà của Claude/Codex được giữ lại.",
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
      "RunDog không tạo tài khoản. CPU, bộ nhớ, GPU và lưu trữ đọc cục bộ qua API Windows. Cài đặt ở HKCU\\Software\\SystemExe\\RunDog.",
      "Có thể đọc JSONL/projects và .credentials.json của Claude, JSONL/sessions và auth.json của Codex, cùng kho LOCALAPPDATA của RunDog. Truy vấn hạn mức không gửi raw prompt/response.",
      "Đích đến: GitHub, Anthropic, chatgpt.com. Không có máy chủ của RunDog. Claude khoảng 5 phút, Codex khoảng 60 giây; hết hạn có thể ghi lại .credentials.json.",
      "Khi có CLI, truy vấn hạn mức tự động vẫn On. Không thêm toggle làm hỏng menu. Không có SDK quảng cáo, phân tích hay sự cố.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  fr: {
    title: "RunDog",
    description:
      "Un chien dans la zone de notification. Sa vitesse indique la charge CPU de Windows.",
    tagline: "Et si vous adoptiez un chien dans la zone de notification ?",
    lead: "La vitesse du chien indique la charge CPU de Windows. Le menu contextuel Icône de notification permet aussi d'afficher CPU, mémoire, GPU, ou les plafonds 5 h et semaine de Claude / Codex plus Fable semaine dans l'icône. Écrit en Rust et optimisé pour n'utiliser presque ni CPU ni mémoire en résidence.",
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
        body: "Plus le CPU est occupé, plus le chien court vite. Au calme, il se promène. Vous pouvez aussi afficher des chiffres via le menu contextuel Icône de notification.",
      },
      {
        title: "Des chiffres dans la barre",
        body: "Icône de notification bascule entre Chien (CPU), CPU, Mémoire, GPU, Claude 5 h, Claude semaine, Fable semaine, Codex 5 h et Codex semaine. Un plafond périmé ou absent affiche --, jamais un 0 % inventé. Le choix est conservé.",
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
    metricsLead: "Pointez le chien ou l'icône chiffrée : la carte s'ouvre. Ces pourcentages peuvent aussi aller dans la barre via le menu contextuel Icône de notification.",
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
        a: "Ce site existe en japonais, anglais, chinois, coréen, vietnamien, français, allemand, espagnol, russe, italien et thaï. Le menu contextuel suit la langue d'affichage de Windows. Les libellés de la carte au survol restent en anglais.",
      },
      {
        q: "La barre peut-elle afficher des chiffres ?",
        a: "Oui. Clic droit Icône de notification pour passer du chien aux pourcentages entiers CPU, Mémoire, GPU, Claude 5 h, Claude semaine, Fable semaine, Codex 5 h et Codex semaine. Un plafond périmé ou absent affiche --. Le choix est enregistré.",
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
        a: "Paramètres Windows → Applications → RunDog. Programme, raccourcis, démarrage, réglages, état d'usage et cache de mise à jour sont retirés. Journaux, identifiants et dossiers Claude/Codex restent.",
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
      "RunDog ne crée pas de compte. CPU, mémoire, GPU et stockage sont lus localement via les API Windows. Réglages : HKCU\\Software\\SystemExe\\RunDog.",
      "Il peut lire les JSONL projects et .credentials.json de Claude, les JSONL sessions et auth.json de Codex, plus le store LOCALAPPDATA de RunDog. Les plafonds n'envoient pas les prompts/réponses bruts.",
      "Destinations : GitHub, Anthropic, chatgpt.com. Pas de serveur RunDog. Claude ~5 min, Codex ~60 s ; un jeton expiré peut réécrire .credentials.json.",
      "Les requêtes automatiques restent activées si les CLI sont présents. Pas de nouveau bascule qui casserait le menu. Aucun SDK pub, analyse ou plantage.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  de: {
    title: "RunDog",
    description:
      "Ein Hund im Infobereich. Wie schnell er läuft, zeigt die CPU-Last von Windows.",
    tagline: "Möchten Sie einen Hund im Infobereich halten?",
    lead: "Wie schnell der Hund läuft, zeigt die CPU-Last von Windows. Über das Rechtsklick-Menü Infobereich können Sie auch CPU, Speicher, GPU oder Claude-/Codex-Limits (5 Std. und Woche) plus Fable-Woche als Zahlen-Icon anzeigen. In Rust optimiert, damit es im Infobereich kaum CPU oder Speicher braucht.",
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
        body: "Je beschäftigter die CPU, desto schneller läuft der Hund. In Ruhe geht er spazieren. Über das Rechtsklick-Menü Infobereich können Sie auch Zahlen ins Icon legen.",
      },
      {
        title: "Zahlen im Infobereich",
        body: "Infobereich wechselt zwischen Hund (CPU), CPU, Speicher, GPU, Claude 5 Std., Claude-Woche, Fable-Woche, Codex 5 Std. und Codex-Woche. Abgelaufene oder fehlende Limits zeigen --, nie ein erfundenes 0 %. Die Einstellung bleibt.",
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
    metricsLead: "Zeigen Sie auf den Hund oder das Zahlen-Icon, und die Karte öffnet sich. Dieselben Prozente können Sie über das Rechtsklick-Menü Infobereich ins Icon legen.",
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
        a: "Diese Seite gibt es auf Japanisch, Englisch, Chinesisch, Koreanisch, Vietnamesisch, Französisch, Deutsch, Spanisch, Russisch, Italienisch und Thai. Das Kontextmenü folgt der Windows-Anzeigesprache. Die Kartenbeschriftung bleibt Englisch.",
      },
      {
        q: "Kann der Infobereich Zahlen zeigen?",
        a: "Ja. Rechtsklick Infobereich wechselt zwischen dem laufenden Hund und ganzen Prozenten für CPU, Speicher, GPU, Claude 5 Std., Claude-Woche, Fable-Woche, Codex 5 Std. und Codex-Woche. Abgelaufene oder fehlende Limits zeigen --. Die Wahl wird gespeichert.",
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
        a: "Windows-Einstellungen → Apps → RunDog. Programm, Verknüpfungen, Autostart, Einstellungen, Nutzungsstand und Update-Cache werden entfernt. Claude- und Codex-Logs, Anmeldedaten und Home-Ordner bleiben.",
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
      "RunDog legt kein Konto an. CPU, Speicher, GPU und Datenträger werden lokal über Windows-APIs gelesen. Einstellungen: HKCU\\Software\\SystemExe\\RunDog.",
      "Es kann Claude-projects-JSONL und .credentials.json, Codex-sessions-JSONL und auth.json sowie den RunDog-LOCALAPPDATA-Store lesen. Limit-Abfragen senden keine Roh-Prompts oder Antworten.",
      "Ziele: GitHub, Anthropic, chatgpt.com. Kein eigener RunDog-Server. Claude ~5 Min., Codex ~60 s; abgelaufene Tokens können .credentials.json neu schreiben.",
      "Automatische Limit-Abfragen bleiben an, wenn die CLIs da sind. Kein neues Menü-Toggle. Kein Werbe-, Analyse- oder Absturz-SDK.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  es: {
    title: "RunDog",
    description:
      "Un perro en el área de notificación. Su velocidad indica la carga de CPU de Windows.",
    tagline: "¿Y si crías un perro en el área de notificación?",
    lead: "La velocidad del perro indica la carga de CPU de Windows. En el menú contextual Icono de notificación también puedes poner CPU, memoria, GPU, o los límites de 5 h y semanales de Claude / Codex más Fable semanal en el icono. Escrito en Rust y optimizado para usar casi nada de CPU ni memoria mientras vive en la bandeja.",
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
        body: "Cuanto más ocupada está la CPU, más rápido corre el perro. En calma, pasea. También puedes poner números en el icono desde el menú contextual Icono de notificación.",
      },
      {
        title: "Números en la bandeja",
        body: "Icono de notificación cambia entre Perro (CPU), CPU, Memoria, GPU, Claude 5 h, Claude semanal, Fable semanal, Codex 5 h y Codex semanal. Un límite caducado o ausente muestra --, nunca un 0 % inventado. La opción se guarda.",
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
    metricsLead: "Apunta al perro o al icono numérico y se abre la tarjeta. Esos porcentajes también pueden ir a la bandeja desde el menú contextual Icono de notificación.",
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
        a: "Este sitio está en japonés, inglés, chino, coreano, vietnamita, francés, alemán, español, ruso, italiano y tailandés. El menú contextual sigue el idioma de Windows. Las etiquetas de la tarjeta al pasar el puntero siguen en inglés.",
      },
      {
        q: "¿La bandeja puede mostrar números?",
        a: "Sí. Clic derecho en Icono de notificación para cambiar entre el perro y porcentajes enteros de CPU, Memoria, GPU, Claude 5 h, Claude semanal, Fable semanal, Codex 5 h y Codex semanal. Un límite caducado o ausente muestra --. La elección se guarda.",
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
        a: "Configuración de Windows → Aplicaciones → RunDog. Se eliminan el programa, accesos, inicio, ajustes, estado de uso y caché de actualizaciones. Los registros, credenciales y carpetas de Claude/Codex se conservan.",
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
      "RunDog no crea una cuenta. CPU, memoria, GPU y almacenamiento se leen en el equipo con las API de Windows. Ajustes: HKCU\\Software\\SystemExe\\RunDog.",
      "Puede leer JSONL de projects y .credentials.json de Claude, JSONL de sessions y auth.json de Codex, y el almacén LOCALAPPDATA de RunDog. La consulta de límites no envía prompts ni respuestas en bruto.",
      "Destinos: GitHub, Anthropic, chatgpt.com. No hay servidor propio de RunDog. Claude ~5 min, Codex ~60 s; un token caducado puede reescribir .credentials.json.",
      "Las consultas automáticas siguen activas si están los CLI. No se añade un interruptor que rompa el menú. Sin SDK de anuncios, analítica ni fallos.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  ru: {
    title: "RunDog",
    description:
      "Собака в области уведомлений. По скорости бега видно нагрузку CPU в Windows.",
    tagline: "Завести собаку в области уведомлений?",
    lead: "Скорость бега показывает загрузку CPU в Windows. В меню правой кнопки «Значок в трее» можно также вывести CPU, память, GPU или лимиты Claude / Codex на 5 ч и неделю плюс Fable за неделю в значок. Написано на Rust и оптимизировано так, чтобы в трее почти не занимать CPU и память.",
    download: "Скачать для Windows",
    requirement: "Windows 10 / 11 (64-бит)",
    viewGithub: "Смотреть на GitHub",
    altTaskbar: "RunDog в области уведомлений",
    altFlyout: "Карточка RunDog при наведении",
    featuresTitle: "Особенности",
    features: [
      {
        title: "Почти без нагрузки",
        body: "Rust и Win32: без GUI-фреймворка и лишних потоков. Release собирается с LTO, чтобы простой CPU и память были минимальными.",
      },
      {
        title: "Нагрузка с одного взгляда",
        body: "Чем занятее CPU, тем быстрее бежит собака. В покое она идёт шагом. Цифры можно вынести в трей через меню «Значок в трее».",
      },
      {
        title: "Цифры в трее",
        body: "«Значок в трее» переключает Собака (CPU), CPU, Память, GPU, Claude 5 ч, Claude за неделю, Fable за неделю, Codex 5 ч и Codex за неделю. Просроченный или отсутствующий лимит — --, выдуманных 0% нет. Выбор сохраняется.",
      },
      {
        title: "Компактная системная карточка",
        body: "CPU, память, GPU и диск — нужное прямо из области уведомлений.",
      },
      {
        title: "Claude и Codex тоже",
        body: "Лимиты подписки и стоимость, эквивалентная API, без запуска CLI.",
      },
    ],
    metricsTitle: "Карточка при наведении",
    metricsLead: "Наведите на собаку или числовой значок — карточка откроется. Те же проценты можно вынести в трей через меню «Значок в трее».",
    metrics: [
      "Загрузка CPU: System / User / Idle",
      "Память",
      "GPU и выделенная / общая видеопамять",
      "Диск",
      "Лимиты Claude Code на 5 часов и 7 дней",
      "Лимиты Codex CLI и стоимость, эквивалентная API",
      "Собственные CPU и память RunDog",
    ],
    usageTitle: "Claude Code и Codex",
    usageBody:
      "На карточке — окна подписки 5 часов и 7 дней плюс стоимость, эквивалентная API. RunDog не запускает claude или codex, а читает только нужные журналы.",
    faqTitle: "Частые вопросы",
    faq: [
      {
        q: "Какие языки поддерживаются?",
        a: "Сайт есть на японском, английском, китайском, корейском, вьетнамском, французском, немецком, испанском, русском, итальянском и тайском. Контекстное меню следует языку интерфейса Windows. Подписи карточки остаются на английском.",
      },
      {
        q: "Может ли трей показывать цифры?",
        a: "Да. Правый щелчок «Значок в трее» переключает бегущую собаку и целые проценты CPU, Память, GPU, Claude 5 ч, Claude за неделю, Fable за неделю, Codex 5 ч и Codex за неделю. Просроченный или отсутствующий лимит — --. Выбор сохраняется.",
      },
      {
        q: "Это то же самое, что RunCat?",
        a: "Нет. RunDog — новое независимое приложение для Windows. Это не замена RunCat, а дань идее бегущего питомца как индикатора нагрузки.",
      },
      {
        q: "Оно тяжёлое?",
        a: "Нет. Rust говорит с Win32 напрямую, без GUI-фреймворка и лишних потоков. С LTO в простое доля CPU машины часто ниже 0,1%, частная память — несколько МиБ.",
      },
      {
        q: "Отправляет ли оно данные наружу?",
        a: "При запуске один раз проверяет GitHub Releases. Если вы пользуетесь Claude или Codex, может запросить их API лимитов с уже имеющимися учётными данными. Нет SDK рекламы или аналитики.",
      },
      {
        q: "Предупредит ли SmartScreen?",
        a: "Подпись SignPath Foundation ещё в заявке. Пока нет Authenticode, Windows может предупредить. Установщик проверяется SHA-256. Ставьте из GitHub-репозитория проекта.",
      },
      {
        q: "Как удалить?",
        a: "Параметры Windows → Приложения → RunDog. Удаляются программа, ярлыки, автозапуск, настройки, состояние usage и кэш обновлений. Журналы, учётные данные и домашние папки Claude/Codex остаются.",
      },
      {
        q: "Какие требования?",
        a: "64-разрядная Windows 10 или 11.",
      },
    ],
    privacy: "Конфиденциальность",
    privacyTitle: "Конфиденциальность",
    back: "Назад к RunDog",
    privacyBody: [
      "RunDog не создаёт учётную запись. CPU, память, GPU и диск читаются локально через API Windows. Настройки: HKCU\\Software\\SystemExe\\RunDog.",
      "Может читать projects JSONL и .credentials.json Claude, sessions JSONL и auth.json Codex, плюс LOCALAPPDATA RunDog. Запрос лимитов не отправляет сырые промпты и ответы.",
      "Адреса: GitHub, Anthropic, chatgpt.com. Своего сервера RunDog нет. Claude примерно каждые 5 мин, Codex — 60 с; при истечении может перезаписать .credentials.json.",
      "Автозапросы лимитов остаются включёнными, если CLI есть. Новый переключатель в меню не добавляли. Нет SDK рекламы, аналитики или сбоев.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  it: {
    title: "RunDog",
    description:
      "Un cane nell'area di notifica. Quanto corre indica il carico CPU di Windows.",
    tagline: "Adottare un cane nell'area di notifica?",
    lead: "La velocità del cane indica il carico CPU di Windows. Dal menu contestuale Icona di notifica puoi anche mettere CPU, memoria, GPU, o i limiti 5 ore e settimanali di Claude / Codex più Fable settimanale nell'icona. Scritto in Rust e ottimizzato per usare quasi zero CPU e memoria mentre resta nel vassoio.",
    download: "Scarica per Windows",
    requirement: "Windows 10 / 11 (64 bit)",
    viewGithub: "Vedi su GitHub",
    altTaskbar: "RunDog nell'area di notifica",
    altFlyout: "Scheda RunDog al passaggio del puntatore",
    featuresTitle: "Caratteristiche",
    features: [
      {
        title: "Quasi nessun overhead",
        body: "Rust su Win32: niente framework GUI né thread extra. Le build di release usano LTO per tenere bassi CPU e memoria a riposo.",
      },
      {
        title: "Il carico a colpo d'occhio",
        body: "Più la CPU è occupata, più il cane corre. Quando è calmo, cammina. Puoi anche mettere i numeri nell'icona dal menu contestuale Icona di notifica.",
      },
      {
        title: "Numeri nel vassoio",
        body: "Icona di notifica passa tra Cane (CPU), CPU, Memoria, GPU, Claude 5 ore, Claude settimanale, Fable settimanale, Codex 5 ore e Codex settimanale. Un limite scaduto o assente mostra --, mai uno 0% inventato. La scelta resta.",
      },
      {
        title: "Una scheda di sistema compatta",
        body: "CPU, memoria, GPU e archiviazione: l'essenziale, dall'area di notifica.",
      },
      {
        title: "Anche Claude e Codex",
        body: "Limiti dell'abbonamento e costo equivalente API, senza avviare le CLI.",
      },
    ],
    metricsTitle: "Scheda al passaggio",
    metricsLead: "Punta il cane o l'icona numerica e la scheda si apre. Quelle percentuali possono andare anche nel vassoio dal menu contestuale Icona di notifica.",
    metrics: [
      "Uso CPU con System / User / Idle",
      "Memoria",
      "GPU con memoria dedicata / condivisa",
      "Archiviazione",
      "Limiti Claude Code di 5 ore e 7 giorni",
      "Limiti Codex CLI e costo equivalente API",
      "CPU e memoria di RunDog stesso",
    ],
    usageTitle: "Claude Code e Codex",
    usageBody:
      "La scheda mostra le finestre di abbonamento da 5 ore e 7 giorni, più il costo equivalente API. RunDog non avvia claude né codex: legge solo i registri necessari.",
    faqTitle: "Domande frequenti",
    faq: [
      {
        q: "Quali lingue sono supportate?",
        a: "Il sito è in giapponese, inglese, cinese, coreano, vietnamita, francese, tedesco, spagnolo, russo, italiano e thai. Il menu contestuale segue la lingua di visualizzazione di Windows. Le etichette della scheda restano in inglese.",
      },
      {
        q: "Il vassoio può mostrare i numeri?",
        a: "Sì. Clic destro su Icona di notifica per passare dal cane ai percentuali interi di CPU, Memoria, GPU, Claude 5 ore, Claude settimanale, Fable settimanale, Codex 5 ore e Codex settimanale. Un limite scaduto o assente mostra --. La scelta viene salvata.",
      },
      {
        q: "È lo stesso di RunCat?",
        a: "No. RunDog è un'app Windows scritta da zero. Non sostituisce RunCat: è un omaggio all'idea di un animale che corre per mostrare il carico.",
      },
      {
        q: "È pesante?",
        a: "No. Parla con Win32 in Rust, senza framework GUI né thread extra. Con LTO, a riposo la CPU della macchina è spesso sotto lo 0,1% e la memoria privata pochi MiB.",
      },
      {
        q: "Invia dati all'esterno?",
        a: "All'avvio controlla GitHub Releases una volta. Se usi Claude o Codex, può interrogare le loro API dei limiti con credenziali già presenti. Nessun SDK di pubblicità o analitica.",
      },
      {
        q: "SmartScreen avviserà?",
        a: "La firma SignPath Foundation è in corso di richiesta. Finché non c'è Authenticode, Windows può avvisare. L'installer è verificato con SHA-256. Installa dal repository GitHub del progetto.",
      },
      {
        q: "Come si disinstalla?",
        a: "Impostazioni Windows → App → RunDog. Si rimuovono programma, collegamenti, avvio, impostazioni, stato usage e cache degli aggiornamenti. Log, credenziali e home di Claude/Codex restano.",
      },
      {
        q: "Requisiti?",
        a: "Windows 10 o 11 a 64 bit.",
      },
    ],
    privacy: "Privacy",
    privacyTitle: "Privacy",
    back: "Torna a RunDog",
    privacyBody: [
      "RunDog non crea un account. CPU, memoria, GPU e archiviazione si leggono in locale tramite le API di Windows. Impostazioni: HKCU\\Software\\SystemExe\\RunDog.",
      "Può leggere JSONL projects e .credentials.json di Claude, JSONL sessions e auth.json di Codex, più lo store LOCALAPPDATA di RunDog. Il fetch dei limiti non invia prompt o risposte grezzi.",
      "Destinazioni: GitHub, Anthropic, chatgpt.com. Nessun server di RunDog. Claude ~5 min, Codex ~60 s; un token scaduto può riscrivere .credentials.json.",
      "Le query automatiche restano attive se i CLI sono presenti. Nessun nuovo interruttore nel menu. Nessun SDK di pubblicità, analitica o crash.",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
  th: {
    title: "RunDog",
    description:
      "สุนัขในพื้นที่แจ้งเตือน ความเร็วที่วิ่งบอกภาระ CPU ของ Windows",
    tagline: "เลี้ยงสุนัขในพื้นที่แจ้งเตือนไหม?",
    lead: "ความเร็วที่สุนัขวิ่งบอกการใช้ CPU ของ Windows จากเมนูคลิกขวาไอคอนถาด คุณยังใส่เปอร์เซ็นต์ CPU / หน่วยความจำ / GPU หรือโควตา 5 ชม. และรายสัปดาห์ของ Claude / Codex รวมถึง Fable รายสัปดาห์ลงในไอคอนได้ เขียนด้วย Rust และปรับให้ตอนอยู่ในถาดระบบแทบไม่กิน CPU หรือหน่วยความจำ",
    download: "ดาวน์โหลดสำหรับ Windows",
    requirement: "Windows 10 / 11 (64 บิต)",
    viewGithub: "ดูบน GitHub",
    altTaskbar: "RunDog ในพื้นที่แจ้งเตือน",
    altFlyout: "การ์ด RunDog เมื่อชี้เมาส์",
    featuresTitle: "จุดเด่น",
    features: [
      {
        title: "แทบไม่เพิ่มภาระ",
        body: "Rust คุยกับ Win32 โดยตรง ไม่มีเฟรมเวิร์ก GUI และไม่มีเธรดเกิน Release ใช้ LTO เพื่อให้ CPU และหน่วยความจำตอนว่างน้อยที่สุด",
      },
      {
        title: "เห็นภาระในพริบตา",
        body: "CPU ยิ่งยุ่ง สุนัขยิ่งวิ่งเร็ว ตอนว่างก็เดินช้า จากเมนูคลิกขวาไอคอนถาด คุณยังใส่ตัวเลขในพื้นที่แจ้งเตือนได้",
      },
      {
        title: "ตัวเลขในถาดระบบ",
        body: "ไอคอนถาดสลับระหว่างสุนัข (CPU) CPU หน่วยความจำ GPU Claude 5 ชม. Claude รายสัปดาห์ Fable รายสัปดาห์ Codex 5 ชม. และ Codex รายสัปดาห์ โควตาที่หมดอายุหรือไม่มีแสดง -- ไม่สร้าง 0% การตั้งค่าจะคงอยู่",
      },
      {
        title: "การ์ดระบบแบบกระชับ",
        body: "CPU หน่วยความจำ GPU และที่เก็บข้อมูล ดูได้จากพื้นที่แจ้งเตือน",
      },
      {
        title: "ดู Claude และ Codex ด้วย",
        body: "โควตาแพ็กเกจและค่าใช้จ่ายเทียบ API โดยไม่ต้องเปิด CLI",
      },
    ],
    metricsTitle: "การ์ดเมื่อชี้เมาส์",
    metricsLead: "ชี้ที่สุนัขหรือไอคอนตัวเลขแล้วการ์ดจะเปิด เปอร์เซ็นต์เดียวกันใส่ในถาดได้จากเมนูคลิกขวาไอคอนถาด",
    metrics: [
      "การใช้ CPU พร้อม System / User / Idle",
      "หน่วยความจำ",
      "GPU พร้อมหน่วยความจำเฉพาะ / ใช้ร่วม",
      "ที่เก็บข้อมูล",
      "โควตา 5 ชั่วโมงและ 7 วันของ Claude Code",
      "โควตา Codex CLI และค่าใช้จ่ายเทียบ API",
      "CPU และหน่วยความจำของตัว RunDog เอง",
    ],
    usageTitle: "Claude Code และ Codex",
    usageBody:
      "การ์ดแสดงหน้าต่างแพ็กเกจ 5 ชั่วโมงและ 7 วัน พร้อมค่าใช้จ่ายเทียบ API RunDog ไม่เปิด claude หรือ codex แค่อ่านล็อกที่จำเป็น",
    faqTitle: "คำถามที่พบบ่อย",
    faq: [
      {
        q: "รองรับภาษาอะไรบ้าง?",
        a: "หน้านี้มีภาษาญี่ปุ่น อังกฤษ จีน เกาหลี เวียดนาม ฝรั่งเศส เยอรมัน สเปน รัสเซีย อิตาลี และไทย เมนูคลิกขวาตามภาษาที่แสดงของ Windows ข้อความบนการ์ดยังเป็นภาษาอังกฤษ",
      },
      {
        q: "ถาดระบบแสดงตัวเลขได้ไหม?",
        a: "ได้ คลิกขวาไอคอนถาดเพื่อสลับระหว่างสุนัขที่วิ่งกับเปอร์เซ็นต์จำนวนเต็มของ CPU หน่วยความจำ GPU Claude 5 ชม. Claude รายสัปดาห์ Fable รายสัปดาห์ Codex 5 ชม. และ Codex รายสัปดาห์ โควตาที่หมดอายุหรือไม่มีแสดง -- ตัวเลือกจะถูกบันทึก",
      },
      {
        q: "เป็นตัวเดียวกับ RunCat หรือไม่?",
        a: "ไม่ใช่ RunDog เป็นแอป Windows ที่เขียนใหม่ ไม่ได้มาแทน RunCat แต่เป็นการคารวะแนวคิดสัตว์เลี้ยงที่วิ่งเพื่อบอกภาระ",
      },
      {
        q: "กินเครื่องไหม?",
        a: "ไม่ Rust พูดกับ Win32 โดยตรง ไม่มีเฟรมเวิร์ก GUI และเธรดเกิน บิลด์ LTO ตอนว่าง CPU ทั้งเครื่องมักต่ำกว่า 0.1% และหน่วยความจำส่วนตัวแค่ไม่กี่ MiB",
      },
      {
        q: "ส่งข้อมูลออกเครื่องไหม?",
        a: "ตอนเริ่มจะเช็ก GitHub Releases ครั้งเดียว ถ้าใช้ Claude หรือ Codex อาจเรียก API โควตาด้วยข้อมูลรับรองที่มีอยู่แล้ว ไม่มี SDK โฆษณาหรือวิเคราะห์",
      },
      {
        q: "SmartScreen จะเตือนไหม?",
        a: "กำลังยื่นลายเซ็น SignPath Foundation ยังไม่มี Authenticode ดังนั้น Windows อาจเตือน ตัวติดตั้งตรวจด้วย SHA-256 ติดตั้งจากที่เก็บ GitHub ของโปรเจกต์",
      },
      {
        q: "ถอนการติดตั้งอย่างไร?",
        a: "การตั้งค่า Windows → แอป → RunDog โปรแกรม ทางลัด การเริ่มอัตโนมัติ การตั้งค่า ข้อมูล usage และแคชอัปเดตจะถูกลบ บันทึก ข้อมูลรับรอง และโฟลเดอร์บ้านของ Claude/Codex จะคงไว้"
      },
      {
        q: "ความต้องการของระบบ?",
        a: "Windows 10 หรือ 11 แบบ 64 บิต",
      },
    ],
    privacy: "ความเป็นส่วนตัว",
    privacyTitle: "ความเป็นส่วนตัว",
    back: "กลับไป RunDog",
    privacyBody: [
      "RunDog ไม่สร้างบัญชี CPU หน่วยความจำ GPU และที่เก็บข้อมูลอ่านในเครื่องผ่าน Windows API การตั้งค่าอยู่ที่ HKCU\\Software\\SystemExe\\RunDog",
      "อาจอ่าน projects JSONL กับ .credentials.json ของ Claude, sessions JSONL กับ auth.json ของ Codex และที่เก็บ LOCALAPPDATA ของ RunDog การขอโควตาไม่ส่งพรอมต์หรือคำตอบดิบ",
      "ปลายทางคือ GitHub, Anthropic, chatgpt.com ไม่มีเซิร์ฟเวอร์ของ RunDog เอง Claude ประมาณ 5 นาที Codex ประมาณ 60 วินาที หมดอายุแล้วอาจเขียน .credentials.json ใหม่",
      "ถ้ามี CLI การถามโควตาอัตโนมัติยังเปิดอยู่ ไม่เพิ่มสวิตช์ที่ทำเมนูพัง ไม่มี SDK โฆษณา วิเคราะห์ หรือรายงานข้อผิดพลาด",
      "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.",
    ],
  },
};
