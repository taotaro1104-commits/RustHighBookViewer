package jp.perfectviewer.rs

/**
 * 画面の文言の翻訳表。日本語の文言をキーに、英語・中国語（簡体）・韓国語・スペイン語を引く。
 * 表に無い文言は、そのまま（日本語で）表示される。`{1}` `{2}` は `Tr.f` で差し込む値。
 */
internal val TRANSLATIONS: Map<String, List<String>> = buildMap {
    // e(日本語, English, 中文, 한국어, Español)
    fun e(ja: String, en: String, zh: String, ko: String, es: String) {
        put(ja, listOf(en, zh, ko, es))
    }

    // --- 共通
    e("やめる", "Cancel", "取消", "취소", "Cancelar")
    e("閉じる", "Close", "关闭", "닫기", "Cerrar")
    e("設定", "Settings", "设置", "설정", "Ajustes")
    e("← 戻る", "← Back", "← 返回", "← 뒤로", "← Atrás")
    e("キャンセルしました", "Cancelled", "已取消", "취소했습니다", "Cancelado")
    e("名前", "Name", "名称", "이름", "Nombre")

    // --- 本の読み込み・状態通知（AppState）
    e("保存先を開けません", "Cannot open the save location", "无法打开保存位置", "저장 위치를 열 수 없습니다", "No se puede abrir el destino de guardado")
    e("ファイルを開けません", "Cannot open the file", "无法打开文件", "파일을 열 수 없습니다", "No se puede abrir el archivo")
    e("画像も書庫（EPUB・ZIP・7z・RAR）も見つかりませんでした", "No images or archives (EPUB, ZIP, 7z, RAR) were found", "未找到图片或压缩包（EPUB、ZIP、7z、RAR）", "이미지도 압축 파일(EPUB·ZIP·7z·RAR)도 찾지 못했습니다", "No se encontraron imágenes ni archivos (EPUB, ZIP, 7z, RAR)")
    e("開けませんでした: {1}", "Could not open: {1}", "无法打开：{1}", "열 수 없었습니다: {1}", "No se pudo abrir: {1}")
    e("（グループ「{1}」）", " (group “{1}”)", "（分组“{1}”）", " (그룹 “{1}”)", " (grupo «{1}»)")
    e("（読めなかった {1} 件は除く）", " ({1} unreadable not included)", "（不含 {1} 个无法读取的）", " (읽지 못한 {1}건 제외)", " (sin contar {1} ilegibles)")
    e("{1} 冊を追加して中止しました", "Added {1} books and stopped", "已添加 {1} 本并中止", "{1}권을 추가하고 중단했습니다", "Se añadieron {1} libros y se detuvo")
    e("{1} 冊を本棚に追加しました", "Added {1} books to the shelf", "已将 {1} 本书加入书架", "{1}권을 책장에 추가했습니다", "Se añadieron {1} libros a la estantería")
    e("文字の設定を適用中…", "Applying text settings…", "正在应用文字设置…", "글자 설정을 적용하는 중…", "Aplicando ajustes de texto…")
    e("この本へのアクセス権がありません。もう一度選び直してください", "No access to this book. Please select it again.", "没有访问这本书的权限，请重新选择", "이 책에 접근할 권한이 없습니다. 다시 선택해 주세요", "No hay acceso a este libro. Vuelve a seleccionarlo.")

    // --- SAF（SafTree）
    e("フォルダを読めません: {1}", "Cannot read the folder: {1}", "无法读取文件夹：{1}", "폴더를 읽을 수 없습니다: {1}", "No se puede leer la carpeta: {1}")
    e("存在しないファイル: {1}", "File does not exist: {1}", "文件不存在：{1}", "존재하지 않는 파일: {1}", "El archivo no existe: {1}")
    e("開けません: {1}", "Cannot open: {1}", "无法打开：{1}", "열 수 없습니다: {1}", "No se puede abrir: {1}")
    e("開けません: {1} ({2})", "Cannot open: {1} ({2})", "无法打开：{1}（{2}）", "열 수 없습니다: {1} ({2})", "No se puede abrir: {1} ({2})")
    e("ファイルが多すぎます（上限 {1}）", "Too many files (limit {1})", "文件太多（上限 {1}）", "파일이 너무 많습니다 (상한 {1})", "Demasiados archivos (límite {1})")

    // --- 「フォルダを選ぶには」ダイアログ（MainActivity）
    e("フォルダを選ぶには", "To choose a folder", "选择文件夹", "폴더를 고르려면", "Para elegir una carpeta")
    e(
        "Android の標準のフォルダ選択では、Download や内部ストレージの最上位は「プライバシー保護のため使用できません」と表示され、選べません。",
        "With Android's standard folder picker, Download and the top level of internal storage show “can't use this folder to protect your privacy” and cannot be chosen.",
        "使用 Android 标准的文件夹选择器时，Download 和内部存储的最上层会提示“为保护隐私，无法使用此文件夹”，无法选择。",
        "Android 기본 폴더 선택기에서는 Download나 내장 저장소 최상위가 “개인정보 보호를 위해 사용할 수 없습니다”라고 표시되어 선택할 수 없습니다.",
        "Con el selector de carpetas estándar de Android, Download y la raíz del almacenamiento interno muestran «no se puede usar esta carpeta para proteger tu privacidad» y no se pueden elegir.",
    )
    e(
        "「すべてのファイルへのアクセス」を許可すると、アプリ内の画面からどのフォルダでも選べます（読み込みも速くなります）。画像の閲覧以外には使いません。",
        "If you allow “All files access”, you can choose any folder from the in-app screen (and loading is faster). It is used only for viewing images.",
        "允许“所有文件访问权限”后，即可在应用内的界面选择任意文件夹（读取也会更快）。仅用于查看图片。",
        "“모든 파일 액세스”를 허용하면 앱 안의 화면에서 어떤 폴더든 고를 수 있습니다 (불러오기도 빨라집니다). 이미지를 보는 용도로만 사용합니다.",
        "Si permites «Acceso a todos los archivos», podrás elegir cualquier carpeta desde la pantalla de la app (y la carga será más rápida). Solo se usa para ver imágenes.",
    )
    e("許可する", "Allow", "允许", "허용", "Permitir")
    e("標準の選択画面", "Standard picker", "标准选择界面", "기본 선택 화면", "Selector estándar")

    // --- 閲覧画面（ReaderScreen）
    e("しおりを外しました: {1} ページ", "Bookmark removed: page {1}", "已移除书签：第 {1} 页", "책갈피를 해제했습니다: {1}페이지", "Marcador quitado: página {1}")
    e("しおりを追加: {1} ページ", "Bookmark added: page {1}", "已添加书签：第 {1} 页", "책갈피를 추가했습니다: {1}페이지", "Marcador añadido: página {1}")
    e("しおりを操作できません: {1}", "Cannot change the bookmark: {1}", "无法操作书签：{1}", "책갈피를 조작할 수 없습니다: {1}", "No se puede modificar el marcador: {1}")
    e("ページを保存しました", "Page saved", "已保存页面", "페이지를 저장했습니다", "Página guardada")
    e("保存に失敗: {1}", "Failed to save: {1}", "保存失败：{1}", "저장 실패: {1}", "Error al guardar: {1}")
    e("この操作は未対応です", "This action is not supported", "暂不支持此操作", "이 동작은 지원하지 않습니다", "Esta acción no es compatible")
    e("表示エラー: {1}", "Display error: {1}", "显示错误：{1}", "표시 오류: {1}", "Error de visualización: {1}")
    e("最後のページです。オートめくりを止めました", "This is the last page. Auto page turning stopped.", "已是最后一页，自动翻页已停止", "마지막 페이지입니다. 자동 넘김을 멈췄습니다", "Es la última página. Se detuvo el paso automático.")
    e("← 本棚", "← Shelf", "← 书架", "← 책장", "← Estantería")
    e("📁 フォルダ参照", "📁 Browse folders", "📁 浏览文件夹", "📁 폴더 찾아보기", "📁 Explorar carpetas")
    e("単ページ", "Single page", "单页", "한 페이지", "Una página")
    e("見開き", "Two pages", "双页", "두 페이지", "Doble página")
    e("自動見開き", "Auto spread", "自动双页", "자동 두 페이지", "Doble automática")
    e("縦スクロール", "Vertical scroll", "纵向滚动", "세로 스크롤", "Desplazamiento vertical")
    e("右綴じ", "Right binding", "右开本", "오른쪽 제본", "Encuadernado a la derecha")
    e("左綴じ", "Left binding", "左开本", "왼쪽 제본", "Encuadernado a la izquierda")
    e("幅", "Width", "宽度", "너비", "Ancho")
    e("高さ", "Height", "高度", "높이", "Alto")
    e("全体", "Whole page", "整页", "전체", "Página completa")
    e("全画面", "Fullscreen", "全屏", "전체 화면", "Pantalla completa")
    e("ページ指定", "Go to page", "跳转页码", "페이지 지정", "Ir a página")
    e("一覧", "Grid", "缩略图", "목록", "Cuadrícula")
    e("文字", "Text", "文字", "글자", "Texto")
    e("色調整", "Color", "色彩调整", "색 조정", "Color")
    e("ルーペ: 押したままなぞって拡大。短くタップで終了", "Magnifier: press and drag to magnify. Tap briefly to exit.", "放大镜：按住并滑动进行放大，轻点一下即可退出", "돋보기: 누른 채로 문질러 확대합니다. 짧게 탭하면 종료합니다", "Lupa: mantén pulsado y desliza para ampliar. Toca brevemente para salir.")
    e("ルーペ", "Magnifier", "放大镜", "돋보기", "Lupa")
    e("自動 {1}秒", "Auto {1}s", "自动 {1} 秒", "자동 {1}초", "Auto {1} s")
    e("自動めくり", "Auto flip", "自动翻页", "자동 넘김", "Paso automático")
    e("オフ", "Off", "关闭", "끄기", "Desactivado")
    e("{1}秒ごと", "Every {1}s", "每 {1} 秒", "{1}초마다", "Cada {1} s")
    e("★ しおり", "★ Bookmark", "★ 书签", "★ 책갈피", "★ Marcador")
    e("☆ しおり", "☆ Bookmark", "☆ 书签", "☆ 책갈피", "☆ Marcador")
    e("保存", "Save", "保存", "저장", "Guardar")

    e("目次", "Contents", "目录", "목차", "Índice")
    // ページ一覧のサムネイルの大きさ
    e("大", "Large", "大", "크게", "Grande")
    e("中", "Medium", "中", "보통", "Mediano")
    e("小", "Small", "小", "작게", "Pequeño")
    // --- 閲覧画面のダイアログ（ReaderDialogs）
    e("しおり", "Bookmarks", "书签", "책갈피", "Marcadores")
    e("★ {1} ページ", "★ Page {1}", "★ 第 {1} 页", "★ {1}페이지", "★ Página {1}")
    e("外す", "Remove", "移除", "해제", "Quitar")
    e("移動", "Go", "跳转", "이동", "Ir")
    e("明るさ", "Brightness", "亮度", "밝기", "Brillo")
    e("コントラスト", "Contrast", "对比度", "대비", "Contraste")
    e("ガンマ", "Gamma", "伽马", "감마", "Gamma")
    e("白黒", "Grayscale", "黑白", "흑백", "Blanco y negro")
    e("セピア", "Sepia", "棕褐色", "세피아", "Sepia")
    e("反転", "Invert", "反色", "반전", "Invertir")
    e("リセット", "Reset", "重置", "초기화", "Restablecer")
    e("ページ一覧  {1}/{2}", "Pages  {1}/{2}", "页面一览  {1}/{2}", "페이지 목록  {1}/{2}", "Páginas  {1}/{2}")

    // --- 設定画面（SettingsScreen）
    e("設定を保存できません: {1}", "Cannot save settings: {1}", "无法保存设置：{1}", "설정을 저장할 수 없습니다: {1}", "No se pueden guardar los ajustes: {1}")
    e("端末の言語", "Device language", "跟随设备语言", "기기 언어", "Idioma del dispositivo")
    e("表示（これから開く本の既定）", "Display (defaults for books you open next)", "显示（今后打开的书的默认设置）", "표시 (앞으로 여는 책의 기본값)", "Visualización (valores predeterminados para los próximos libros)")
    e("すでに開いたことのある本は、その本ごとの設定が優先されます。", "Books you have already opened use their own settings.", "已经打开过的书，优先使用该书自己的设置。", "이미 열어 본 책은 그 책의 설정이 우선됩니다.", "Los libros que ya abriste usan sus propios ajustes.")
    e("表示モード", "View mode", "显示模式", "표시 모드", "Modo de vista")
    e("綴じ方向", "Binding direction", "装订方向", "제본 방향", "Sentido de encuadernación")
    e("フィット", "Fit", "适应方式", "맞춤", "Ajuste")
    e("表紙を単独で表示", "Show the cover alone", "封面单独显示", "표지를 단독으로 표시", "Mostrar la portada sola")
    e("先頭ページだけ 1 枚で表示します", "Only the first page is shown on its own.", "仅首页单独显示。", "첫 페이지만 한 장으로 표시합니다.", "Solo la primera página se muestra sola.")
    e("横長のページは単独で表示", "Show wide pages alone", "横向页面单独显示", "가로로 긴 페이지는 단독 표시", "Mostrar solas las páginas apaisadas")
    e("見開きの絵などを 2 ページ分に割り込ませません", "Keeps double-page artwork from being split across two pages.", "避免跨页插图被拆到两个页面中。", "펼침면 그림 등이 두 페이지에 걸쳐 나뉘지 않게 합니다.", "Evita que las ilustraciones a doble página se repartan en dos páginas.")
    e("操作", "Controls", "操作", "조작", "Controles")
    e("音量キーでページ送り", "Turn pages with volume keys", "用音量键翻页", "볼륨 키로 페이지 넘기기", "Pasar página con las teclas de volumen")
    e("音量＋で前へ、音量－で次へ（綴じ方向に従います）", "Volume up goes back, volume down goes forward (follows the binding direction).", "音量＋上一页，音量－下一页（遵循装订方向）。", "볼륨 +는 이전, 볼륨 -는 다음 (제본 방향에 따릅니다).", "Subir volumen retrocede y bajar avanza (según el sentido de encuadernación).")
    e("閲覧中は画面を点灯したままにする", "Keep the screen on while reading", "阅读时保持屏幕常亮", "읽는 동안 화면 켜 두기", "Mantener la pantalla encendida al leer")
    e("オートめくり中は、この設定にかかわらず点灯します", "The screen stays on during auto page turning regardless of this setting.", "自动翻页时无论是否开启此项，屏幕都会常亮。", "자동 넘김 중에는 이 설정과 관계없이 화면이 켜져 있습니다.", "Durante el paso automático la pantalla permanece encendida, sin importar este ajuste.")
    e("本棚", "Shelf", "书架", "책장", "Estantería")
    e("「すべて」「お気に入り」の並び順", "Order of “All” and “Favorites”", "“全部”和“收藏”的排序", "“전체”·“즐겨찾기” 정렬 순서", "Orden de «Todos» y «Favoritos»")
    e("最近開いた順", "Recently opened", "最近打开", "최근 연 순", "Abiertos recientemente")
    e("追加順", "Date added", "添加顺序", "추가한 순", "Fecha de adición")
    e("サイズ", "Size", "大小", "크기", "Tamaño")
    e("キャッシュ（メモリ）", "Cache (memory)", "缓存（内存）", "캐시 (메모리)", "Caché (memoria)")
    e("変更は、次に本を開いたときから反映されます。", "Changes take effect the next time you open a book.", "更改将在下次打开书时生效。", "변경 사항은 다음에 책을 열 때부터 적용됩니다.", "Los cambios se aplican la próxima vez que abras un libro.")
    e("画像キャッシュの上限", "Image cache limit", "图片缓存上限", "이미지 캐시 한도", "Límite de caché de imágenes")
    e("自動", "Auto", "自动", "자동", "Automático")
    e("先読み（次のページ側）", "Prefetch (next pages)", "预读（后续页）", "미리 읽기 (다음 페이지 쪽)", "Precarga (páginas siguientes)")
    e("先読み（前のページ側）", "Prefetch (previous pages)", "预读（前面的页）", "미리 읽기 (이전 페이지 쪽)", "Precarga (páginas anteriores)")
    e("{1}枚", "{1} pages", "{1} 页", "{1}장", "{1} págs.")
    e("データ", "Data", "数据", "데이터", "Datos")
    e("本ごとの設定をすべて初期化", "Reset all per-book settings", "重置所有书籍的单独设置", "모든 책별 설정 초기화", "Restablecer los ajustes de todos los libros")
    e("各本に保存された表示モード・綴じ方向・フィット・色調整を消し、上の既定が使われるようにします。しおりと読書位置は消えません。", "Clears the view mode, binding direction, fit and color settings saved for each book so the defaults above are used. Bookmarks and reading positions are kept.", "清除每本书保存的显示模式、装订方向、适应方式和色彩调整，改用上面的默认设置。书签和阅读位置不会被清除。", "각 책에 저장된 표시 모드·제본 방향·맞춤·색 조정을 지우고 위의 기본값을 사용하게 합니다. 책갈피와 읽던 위치는 지워지지 않습니다.", "Borra el modo de vista, el sentido de encuadernación, el ajuste y el color guardados en cada libro para usar los valores predeterminados de arriba. Los marcadores y la posición de lectura se conservan.")
    e("このアプリについて", "About this app", "关于此应用", "이 앱에 대해", "Acerca de esta app")
    e("対応形式: ZIP / CBZ、7z、RAR、EPUB（画像・文章）、画像フォルダ。MIT License。RAR の展開には UnRAR を使用しています。", "Supported formats: ZIP / CBZ, 7z, RAR, EPUB (image-based and text), image folders. MIT License. UnRAR is used to extract RAR files.", "支持的格式：ZIP / CBZ、7z、RAR、EPUB（图片型和文字型）、图片文件夹。MIT 许可证。使用 UnRAR 解压 RAR 文件。", "지원 형식: ZIP / CBZ, 7z, RAR, EPUB(이미지·문장), 이미지 폴더. MIT License. RAR 압축 해제에는 UnRAR를 사용합니다.", "Formatos admitidos: ZIP / CBZ, 7z, RAR, EPUB (de imágenes y de texto), carpetas de imágenes. Licencia MIT. Se usa UnRAR para extraer RAR.")
    e("本ごとの設定を初期化しますか？", "Reset per-book settings?", "要重置每本书的设置吗？", "책별 설정을 초기화할까요?", "¿Restablecer los ajustes de cada libro?")
    e("すべての本の表示設定が、既定に戻ります。", "The display settings of all books will return to the defaults.", "所有书籍的显示设置将恢复为默认值。", "모든 책의 표시 설정이 기본값으로 돌아갑니다.", "Los ajustes de visualización de todos los libros volverán a los valores predeterminados.")
    e("初期化しました", "Reset done", "已重置", "초기화했습니다", "Restablecido")
    e("初期化できません: {1}", "Cannot reset: {1}", "无法重置：{1}", "초기화할 수 없습니다: {1}", "No se puede restablecer: {1}")
    e("初期化", "Reset", "重置", "초기화", "Restablecer")

    // --- 本棚（ShelfScreen）
    e("履歴", "History", "历史", "기록", "Historial")
    e("すべて", "All", "全部", "전체", "Todos")
    e("お気に入り", "Favorites", "收藏", "즐겨찾기", "Favoritos")
    e("グループ", "Groups", "分组", "그룹", "Grupos")
    e("登録中 {1}/{2}… キャンセル", "Registering {1}/{2}… Cancel", "正在登记 {1}/{2}… 取消", "등록 중 {1}/{2}… 취소", "Registrando {1}/{2}… Cancelar")
    e("読み込み中… キャンセル", "Loading… Cancel", "读取中… 取消", "불러오는 중… 취소", "Cargando… Cancelar")
    e("ファイル", "File", "文件", "파일", "Archivo")
    e("フォルダ", "Folder", "文件夹", "폴더", "Carpeta")
    e("▦ サムネイル表示", "▦ Thumbnails", "▦ 缩略图显示", "▦ 썸네일 표시", "▦ Miniaturas")
    e("☰ 名前の一覧", "☰ Name list", "☰ 名称列表", "☰ 이름 목록", "☰ Lista de nombres")
    e("{1} 冊を選択中", "{1} selected", "已选择 {1} 本", "{1}권 선택 중", "{1} seleccionados")
    e("グループに追加", "Add to group", "加入分组", "그룹에 추가", "Añadir al grupo")
    e("タイトル検索", "Search titles", "搜索标题", "제목 검색", "Buscar títulos")
    e("本がありません。「ファイル」から ZIP / CBZ / 7z / RAR / EPUB を、「フォルダ」から画像フォルダを選んでください。", "No books yet. Choose ZIP / CBZ / 7z / RAR / EPUB with “File”, or an image folder with “Folder”.", "还没有书。请通过“文件”选择 ZIP / CBZ / 7z / RAR / EPUB，或通过“文件夹”选择图片文件夹。", "책이 없습니다. “파일”에서 ZIP / CBZ / 7z / RAR / EPUB를, “폴더”에서 이미지 폴더를 선택해 주세요.", "Aún no hay libros. Elige ZIP / CBZ / 7z / RAR / EPUB con «Archivo», o una carpeta de imágenes con «Carpeta».")
    e(" 読書中", " Reading", " 阅读中", " 읽는 중", " Leyendo")
    e(" 読了", " Finished", " 已读完", " 읽음", " Terminado")
    e("選んで、まとめてグループへ", "Select several to group", "选择多本并加入分组", "여러 권을 선택해 그룹으로", "Seleccionar varios para agrupar")
    e("このグループから外す", "Remove from this group", "从此分组移除", "이 그룹에서 제외", "Quitar de este grupo")
    e("お気に入りを外す", "Remove from favorites", "取消收藏", "즐겨찾기 해제", "Quitar de favoritos")
    e("お気に入りにする", "Add to favorites", "加入收藏", "즐겨찾기에 추가", "Añadir a favoritos")
    e("ライブラリから削除", "Delete from library", "从库中删除", "라이브러리에서 삭제", "Eliminar de la biblioteca")

    // --- フォルダ表示（ShelfFolders）
    e("配下すべて表示", "Include subfolders", "显示所有子文件夹内容", "하위 폴더까지 모두 표시", "Incluir subcarpetas")
    // --- 並び替え・読書状況（ShelfSort）
    e("未読", "Unread", "未读", "읽지 않음", "Sin leer")
    e("読書中", "Reading", "阅读中", "읽는 중", "Leyendo")
    e("既読", "Finished", "已读", "읽음", "Terminado")
    e("並び替え: {1} {2}", "Sort: {1} {2}", "排序：{1} {2}", "정렬: {1} {2}", "Orden: {1} {2}")
    e("昇順にする（古い・小さい順）", "Ascending (oldest / smallest first)", "升序（旧的、小的在前）", "오름차순 (오래된 순·작은 순)", "Ascendente (más antiguos / pequeños primero)")
    e("降順にする（新しい・大きい順）", "Descending (newest / largest first)", "降序（新的、大的在前）", "내림차순 (최신 순·큰 순)", "Descendente (más recientes / grandes primero)")
    e("並び順", "Sort order", "排序方式", "정렬 순서", "Criterio de orden")
    e("並びの向き", "Sort direction", "排序方向", "정렬 방향", "Sentido del orden")
    e("昇順", "Ascending", "升序", "오름차순", "Ascendente")
    e("降順", "Descending", "降序", "내림차순", "Descendente")
    // --- グループ（ShelfGroups）
    e("グループがありません。本を長押しして「グループに追加」するか、複数の本が入ったフォルダを読み込むと作られます。", "No groups yet. Long-press a book and choose “Add to group”, or import a folder that contains several books.", "还没有分组。长按书籍选择“加入分组”，或导入包含多本书的文件夹即可创建。", "그룹이 없습니다. 책을 길게 눌러 “그룹에 추가”하거나, 여러 권이 들어 있는 폴더를 불러오면 만들어집니다.", "Aún no hay grupos. Mantén pulsado un libro y elige «Añadir al grupo», o importa una carpeta con varios libros.")
    e("{1} 冊", "{1} books", "{1} 本", "{1}권", "{1} libros")
    e("名前を変更", "Rename", "重命名", "이름 변경", "Cambiar nombre")
    e("グループを解除（本は残る）", "Dissolve group (books are kept)", "解散分组（书会保留）", "그룹 해제 (책은 남습니다)", "Disolver grupo (los libros se conservan)")
    e("{1} 冊を「{2}」に追加しました", "Added {1} books to “{2}”", "已将 {1} 本书加入“{2}”", "{1}권을 “{2}”에 추가했습니다", "Se añadieron {1} libros a «{2}»")
    e("グループに追加できませんでした", "Could not add to the group", "无法加入分组", "그룹에 추가할 수 없었습니다", "No se pudo añadir al grupo")
    e("グループに追加（{1} 冊）", "Add to group ({1} books)", "加入分组（{1} 本）", "그룹에 추가 ({1}권)", "Añadir al grupo ({1} libros)")
    e("新しいグループ名", "New group name", "新分组名称", "새 그룹 이름", "Nombre del nuevo grupo")
    e("グループを作れませんでした", "Could not create the group", "无法创建分组", "그룹을 만들 수 없었습니다", "No se pudo crear el grupo")
    e("作って追加", "Create and add", "创建并加入", "만들어서 추가", "Crear y añadir")
    e("グループ名を変更", "Rename group", "重命名分组", "그룹 이름 변경", "Cambiar nombre del grupo")
    e("変更できませんでした（同じ名前のグループがあるかもしれません）", "Could not rename (a group with the same name may already exist)", "无法重命名（可能已有同名分组）", "변경할 수 없었습니다 (같은 이름의 그룹이 있을 수 있습니다)", "No se pudo cambiar el nombre (puede que ya exista un grupo con ese nombre)")
    e("変更", "Rename", "修改", "변경", "Cambiar")
    e("グループを解除", "Dissolve group", "解散分组", "그룹 해제", "Disolver grupo")
    e("「{1}」をやめます。中の本はライブラリに残ります。", "“{1}” will be dissolved. The books inside stay in the library.", "将解散“{1}”。其中的书会保留在库中。", "“{1}” 그룹을 해제합니다. 안의 책은 라이브러리에 남습니다.", "Se disolverá «{1}». Los libros que contiene permanecen en la biblioteca.")
    e("解除する", "Dissolve", "解散", "해제", "Disolver")
    e("解除", "Dissolve", "解散", "해제", "Disolver")
    e("← グループ", "← Groups", "← 分组", "← 그룹", "← Grupos")

    // --- フォルダ選択（FolderBrowserScreen）
    e("フォルダを選ぶ", "Choose a folder", "选择文件夹", "폴더 선택", "Elegir una carpeta")
    e("このフォルダを読み込む", "Import this folder", "读取此文件夹", "이 폴더 불러오기", "Importar esta carpeta")
    e("↑ 上へ", "↑ Up", "↑ 上一级", "↑ 위로", "↑ Subir")
    e("読み込み中…", "Loading…", "读取中…", "불러오는 중…", "Cargando…")
    e("このフォルダは読めません", "This folder cannot be read", "无法读取此文件夹", "이 폴더는 읽을 수 없습니다", "No se puede leer esta carpeta")
    e("このフォルダの画像: {1} 枚。画像は 1 冊に、EPUB などの書庫は 1 ファイルごとに 1 冊として、サブフォルダも含めて本棚に追加します", "Images in this folder: {1}. Images become one book, and each archive such as EPUB becomes its own book; subfolders are included and added to the shelf.", "此文件夹中的图片：{1} 张。图片合为一本书，EPUB 等压缩包每个文件算一本书，包含子文件夹，一并加入书架。", "이 폴더의 이미지: {1}장. 이미지는 한 권으로, EPUB 같은 압축 파일은 파일 하나당 한 권으로 하위 폴더까지 포함해 책장에 추가합니다.", "Imágenes en esta carpeta: {1}. Las imágenes forman un libro y cada archivo como EPUB es otro libro; se incluyen las subcarpetas y todo se añade a la estantería.")
    e("サブフォルダはありません", "No subfolders", "没有子文件夹", "하위 폴더가 없습니다", "No hay subcarpetas")
    e("画像 {1} 枚", "{1} images", "{1} 张图片", "이미지 {1}장", "{1} imágenes")
    e("読み込む", "Import", "读取", "불러오기", "Importar")
    e("内部ストレージ", "Internal storage", "内部存储", "내장 저장소", "Almacenamiento interno")
    e("SD カード", "SD card", "SD 卡", "SD 카드", "Tarjeta SD")

    // --- 文字の設定（TextStyleDialog）
    e("文字の色", "Text color", "文字颜色", "글자 색", "Color del texto")
    e("背景の色", "Background color", "背景颜色", "배경 색", "Color de fondo")
    e("文字の色と背景の色が近く、読みにくくなります。", "The text and background colors are too close, so it will be hard to read.", "文字颜色与背景颜色太接近，会难以阅读。", "글자 색과 배경 색이 비슷해서 읽기 어렵습니다.", "El color del texto y el del fondo son muy parecidos y costará leer.")
    e("吾輩は猫である。名前はまだ無い。", "I am a cat. As yet I have no name.", "吾輩是猫。名字还没有。", "나는 고양이로소이다. 이름은 아직 없다.", "Soy un gato. Aún no tengo nombre.")
    e("文字の向き", "Text direction", "文字方向", "글자 방향", "Dirección del texto")
    e("縦書き", "Vertical", "竖排", "세로쓰기", "Vertical")
    e("横書き", "Horizontal", "横排", "가로쓰기", "Horizontal")
    e("文字間  {1}%", "Letter spacing  {1}%", "字间距  {1}%", "글자 간격  {1}%", "Espaciado de letras  {1}%")
    e("文字間", "Letter spacing", "字间距", "글자 간격", "Espaciado de letras")
    e("詰める", "Tighter", "收紧", "좁게", "Juntar")
    e("空ける", "Wider", "加宽", "넓게", "Separar")
    e("太字", "Bold", "粗体", "굵게", "Negrita")
    e("斜体", "Italic", "斜体", "기울임", "Cursiva")
    e("行間  {1}%", "Line spacing  {1}%", "行距  {1}%", "줄 간격  {1}%", "Interlineado  {1}%")
    e("行間", "Line spacing", "行距", "줄 간격", "Interlineado")
    e("狭く", "Tighter", "缩小", "좁게", "Menos")
    e("広く", "Looser", "放大", "넓게", "Más")
    e("文章の本（EPUB）", "Text books (EPUB)", "文字书籍（EPUB）", "문장 책 (EPUB)", "Libros de texto (EPUB)")
    e(
        "画像の無い文章の EPUB を表示するときの見た目です。変更は、次に本を開いたときから反映されます（開いている本は、閲覧メニューの「文字」で開き直せます）。",
        "How text-only EPUBs are displayed. Changes apply the next time you open a book (an open book can be reloaded with “Text” in the reader menu).",
        "没有图片的纯文字 EPUB 的显示外观。更改将在下次打开书时生效（已打开的书可通过阅读菜单中的“文字”重新载入）。",
        "이미지가 없는 문장 EPUB을 표시할 때의 모양입니다. 변경 사항은 다음에 책을 열 때부터 적용됩니다 (열려 있는 책은 열람 메뉴의 “글자”로 다시 열 수 있습니다).",
        "Aspecto de los EPUB de solo texto. Los cambios se aplican la próxima vez que abras un libro (un libro abierto se recarga con «Texto» en el menú del lector).",
    )
    e("文字の大きさ", "Text size", "文字大小", "글자 크기", "Tamaño del texto")
    e("ゴシック体", "Gothic", "黑体", "고딕체", "Gótica")
    e("明朝体", "Mincho (serif)", "宋体", "명조체", "Mincho (serifa)")
    e("文字の設定", "Text settings", "文字设置", "글자 설정", "Ajustes de texto")
    e("文字の大きさ  {1}", "Text size  {1}", "文字大小  {1}", "글자 크기  {1}", "Tamaño del texto  {1}")
    e("小さく", "Smaller", "缩小", "작게", "Menor")
    e("大きく", "Larger", "放大", "크게", "Mayor")
    e("書体", "Typeface", "字体", "글꼴", "Tipo de letra")
    e("この端末には、ほかの書体のフォントがありません。", "No other typefaces are available on this device.", "此设备上没有其他字体。", "이 기기에는 다른 글꼴이 없습니다.", "No hay otros tipos de letra en este dispositivo.")
    e("適用", "Apply", "应用", "적용", "Aplicar")
    e("フォントを取り込む", "Import a font", "导入字体", "글꼴 가져오기", "Importar una fuente")
    e("ttf / otf / ttc（64 MB まで）。日本語の字形が無い文字は、ゴシック体で表示されます。", "ttf / otf / ttc (up to 64 MB). Characters the font lacks are shown in Gothic.", "ttf / otf / ttc（最大 64 MB）。字体中没有的日文字符将以黑体显示。", "ttf / otf / ttc (최대 64 MB). 글꼴에 없는 글자는 고딕체로 표시됩니다.", "ttf / otf / ttc (hasta 64 MB). Los caracteres que falten se muestran en Gótica.")
    e("取り込んだフォント", "Imported fonts", "已导入的字体", "가져온 글꼴", "Fuentes importadas")
    e("削除", "Delete", "删除", "삭제", "Eliminar")
    e("このフォントを削除しますか？", "Delete this font?", "要删除此字体吗？", "이 글꼴을 삭제할까요?", "¿Eliminar esta fuente?")
    e("フォントを取り込めません: {1}", "Could not import the font: {1}", "无法导入字体：{1}", "글꼴을 가져올 수 없습니다: {1}", "No se pudo importar la fuente: {1}")
    e("フォントを削除できません: {1}", "Could not delete the font: {1}", "无法删除字体：{1}", "글꼴을 삭제할 수 없습니다: {1}", "No se pudo eliminar la fuente: {1}")
    e("ファイルが大きすぎます（64 MB まで）", "The file is too large (64 MB max)", "文件过大（最大 64 MB）", "파일이 너무 큽니다 (최대 64 MB)", "El archivo es demasiado grande (máx. 64 MB)")
    e("ファイルを開けません", "Could not open the file", "无法打开文件", "파일을 열 수 없습니다", "No se pudo abrir el archivo")
}
