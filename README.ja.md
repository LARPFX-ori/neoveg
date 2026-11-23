# Neoveg

[Português (BR)](README.md) | [English](README.en.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | **日本語**

**純粋な Rust** で書かれたノンリニア動画編集ソフト (NLE) です。ネイティブ多言語インターフェイス（デフォルト `pt-BR`、`en`、`es`、`fr`、`de`、`ja`；`eframe` + `wgpu`、`glow` への自動フォールバック対応）、VEGAS Pro に着想を得た操作体系、そして**本番環境でのゼロパニック**という厳格な方針を備えています。

## Neoveg を試しましたか？感想をお聞かせください

Neoveg をインストールまたは使用された方は、ぜひ **[Neoveg テスター向けフィードバックフォーム](https://docs.google.com/forms/d/e/1FAIpQLSfal0jWd9rdw7ph_Zm9_ZsTrZiThVsuzjlGMjL9v37Bdq9asQ/viewform)** にご回答ください。いただいたご意見は、今後の修正や優先開発項目の決定に役立てられます。再現手順を添えた不具合報告は、リポジトリの *Issues* でも受け付けています。

## スクリーンショット

![デモプロジェクトを開いた Neoveg のメインウィンドウ：プロジェクト メディア、ビデオ プレビュー、マスター バス、Encoder ボタン、およびビデオ・タイトル・オーディオトラック、自動クロスフェード、マーカー、リージョンを含むタイムライン](docs/screenshots/janela_principal.png)

| ようこそ画面 | 名前を付けてレンダリング... |
|---|---|
| ![オリジナルバナー、5種類のアスペクト比、解像度、フレームレート選択を備えた Neoveg のようこそ画面](docs/screenshots/boas_vindas.png) | ![検索、お気に入り、テンプレート一覧、Encoder に送るボタンを備えた名前を付けてレンダリング画面](docs/screenshots/renderizar_como.png) |

| ビデオ FX | メディア ジェネレーター |
|---|---|
| ![カテゴリー、検索、エフェクト一覧、プリセットサムネイルを備えたビデオ FX ウィンドウ](docs/screenshots/efeitos_de_video.png) | ![カテゴリー、ジェネレーター種類、プリセットサムネイルを備えたメディア ジェネレーター ウィンドウ](docs/screenshots/geradores_de_midia.png) |

| カラーグレーディング | ビデオスコープ |
|---|---|
| ![ベクターカラーホイールとプレビュー連動プリセットを備えたカラーグレーディング パネル](docs/screenshots/color_grading.png) | ![RGB + Luma ヒストグラムと波形を表示するビデオスコープ](docs/screenshots/escopos_de_video.png) |

| ミキサーと補助バス | 5.1 サラウンド パンナー |
|---|---|
| ![トラックフェーダー、2つの補助バス、マスター バスを備えたマルチトラックミキサー](docs/screenshots/mixer_e_barramentos.png) | ![ITU-R BS.775 スピーカーリングと LFE レベルを備えた 5.1 サラウンド パンナー](docs/screenshots/surround_panner.png) |

| ラウドネス メーター (EBU R128) | イベントのプロパティ |
|---|---|
| ![M/S/I/True Peak バーとオフライン解析を備えた EBU R128 ラウドネス メーター](docs/screenshots/medidores_loudness.png) | ![イベントのプロパティ：名前、開始位置、長さ、メディアオフセット、速度、フェード、オプション](docs/screenshots/propriedades_do_evento.png) |

![エンコードキュー、ライブエンコードパネル、プリセットブラウザー、監視フォルダーを表示する分離ウィンドウの Neoveg Encoder](docs/screenshots/neoveg_encoder.png)

スクリーンショットには Neoveg のインターフェイスのみを使用しており、メディアはすべてプログラム自身が生成した合成素材（グラデーション、カラーバー、テキスト、正弦波オーディオ）です。

## ワークスペース構成（5 クレート）

- **`neoveg_common`** (`crates/neoveg_common`): 基本型、正確な整数時間表現（`Ticks`、`FrameRate`、`Rate`、*SMPTE Drop-Frame*）、カーブ、ユーザー設定および共通ユーティリティ。
- **`neoveg_engine`** (`crates/neoveg_engine`): タイムライン状態、アトミック編集不変条件、最大 26 本の補助バス（`Bus A`..`Bus Z`）を備えたマルチトラックミキサー、`fundsp` による DSP 処理、`rubato` による *varispeed*、2D/3D ビデオコンポジター（アニメーション可能なベジェマスク付きパン/クロップ、シャドウ/グロー付き 2D/3D トラックモーション、3D 親子階層、Z 深度ソート、*off-axis* ステレオスコピック 3D カメラ）、エフェクト/トランジション、スコープ、Premiere Pro 風タブ形式の内部シーケンス（`Project.sequences` と単独 `.nvegseq`）、ネストプロジェクト復旧機能付き `.neoveg`/`.nvp` 永続化、クリーンルーム実装の交換形式（OpenTimelineIO、FCPXML、FCP7 XML、EDL CMX 3600、AAF、SRT/WebVTT）、5.1 *サラウンド*、隔離プラグインホストによる OFX（タイル描画、補助入力対応 *General* コンテキスト、ジェネレーター）および VST3（`IPlugView`、サンプル精度オートメーション、クリーンルーム SMF `.mid`/`.midi` パーサー付き MIDI インストゥルメント）、純 Rust による音声・映像デコード（`WAV`/`AIFF`/`FLAC`/`Vorbis`/`MP3`、`FFV1 v3`、`media/fallback.rs`）、ネイティブ **Windows Media Foundation**・**Linux VA-API**・**macOS VideoToolbox/AudioToolbox** バックエンドを統合するシステムデコード契約（`media/system.rs`）、および **単一コマンドテーブル**。
- **`neoveg_scripting`** (`crates/neoveg_scripting`): ヘッドレス自動化（`--command`、`--list-commands`）および `netcorehost` を用いた `ScriptPortal.Vegas` 互換（クリーンルーム、プロトコル 6）`.NET`/`C#` スクリプト（`.cs`/`.dll`）のオンデマンドブリッジ。
- **`neoveg_ui`** (`crates/neoveg_ui`): `eframe` + `wgpu` によるネイティブデスクトップ UI、22 種類のオリジナルベクター編集カーソル付きインタラクティブタイムライン（`cursors.rs`）、マルチタブ分離ウィンドウ対応の柔軟な `egui_tiles` ドッキング、ダイアログの永続ドッキング、`egui_kittest` による操作テスト、7 種類のビジュアルテーマ（WCAG AAA ハイコントラスト 2 種を含む）、`.neoveglayout` レイアウトマネージャー、CC1–CC4 クローズドキャプションおよびインタラクティブ 360° 正距円筒ビュー対応プレビュー/スコープ、ショートカットおよびユーザー設定エディター。
- **`neoveg`** (`crates/neoveg`): メインバイナリおよび CLI（ハードウェアに応じた `rayon` スレッドプール初期化を含む）。

## 実装済み機能

- **VEGAS Pro 準拠のタイムライン・オーディオ・編集:** マルチ A/V トラック、最大 26 補助バス（`Bus A`..`Bus Z`）＋*マスター バス*（`Mono`/`Dim -20 dB`、*pre/post-fader* センド、非循環ルーティング）を備えたミキサー、`fundsp` DSP、同一トラック上でのイベント重ね合わせによる自動クロスフェード、イベント上隅のフェードハンドル、オーディオイベント上端の dB ゲインハンドル、`1 px` 物理再生ヘッド、連続再生速度制御（`rubato` による *Sinc* リサンプリング付き *Playback Rate*）、分割（`S`）、マーカー（`M`）、リージョン（`R`）、*Auto Ripple*（`Ctrl+L`）、スナップ（`F8`）、*スクラブ*（`J`/`K`/`L`）。*Adjustment Track*（`insert.track_adjustment`）、5.1 *サラウンド パンナー*（ステレオ/5.1 マスター、LFE、ITU ダウンミックス、6ch WAV レンダリング）、EBU R128 ラウドネス メーター、および **プロジェクト作業フォルダー**（*ファイル → 作業フォルダー...*、`file.work_folders` によりキャプチャ、録音、プリレンダリング、プロキシ、書き出し先を永続設定）。
- **タイムライン上のトラック キーフレーム:** トラックのビデオエフェクトおよび *Track Motion* のキーフレームをビデオトラック下部に表示。ドラッグで位置移動（スナップ対応・隣接キーの追い越し防止）、右クリックで追加・削除および 6 種類のクラシック補間カーブ（*リニア、高速、低速、スムーズ、シャープ、ホールド* — Track Motion・エフェクトパラメーター両対応）を切り替え可能。トラックヘッダーのキーフレームボタンで展開・折りたたみできます（VEGAS Pro 21 マニュアル p. 389）。
- **イベントごとの複数テイク:** 1 つのイベントに複数の代替メディア（テイク）を個別のオフセットと名前付きで保持可能。プロジェクト メディアの右クリックメニューから *選択イベントにテイクとして追加* すると既存テイクを保ったまま新しいメディアがアクティブテイクになり、`T`/`Shift+T` で前後を切り替え、イベントの *テイク* サブメニューから選択・削除でき、*オプション → すべてのテイクをスリップ* で全テイクを同期スリップします。分割・トリム時もテイクは同期され、レンダリングや書き出しではアクティブテイクが使われます（VEGAS Pro 21 マニュアル pp. 346–348）。
- **外部プラグイン（OFX・VST3）:** 標準フォルダーおよびユーザー指定フォルダー（Google Drive 等、*プラグイン マネージャー → プラグインフォルダー*）から VST3/OFX を自動検出し、*プラグイン マネージャー* およびチェーンピル付き 2 カラム *Plug-In Chooser* で管理。**OFX** ビデオエフェクト・ジェネレーター（`SupportsTiles = 1` タイルレンダリング、複数補助入力対応の *General* コンテキスト、*メディア ジェネレーター → サードパーティ* 内の *Filter*/*Generator*、パラメーターキーフレーム、`getValueAtTime`/`getDerivative`/`getIntegral` によるホスト評価）および **VST3** エフェクト・インストゥルメント（イベント/トラック/バス/マスター対応、ネイティブ `IPlugView`/`IPlugFrame` GUI、`IParameterChanges` によるリアルタイムサンプル精度オートメーション、`IEventList` とクリーンルーム SMF `.mid`/`.midi` パーサーによる MIDI インストゥルメント、`IComponent::getState`/`setState` 状態保存、自動レイテンシ補正 PDC）を隔離サブプロセス `neoveg --plugin-host` で実行するため、プラグイン障害でエディター本体が落ちません。
- **単一コマンドテーブル・ショートカット・`.NET` スクリプト:** メニュー、ツールバー、キーボードショートカット、CLI コマンドがすべて同一のコマンド `id` 経由で実行されます。*ヘルプ → キーボード ショートカット* でカスタム割り当て・解除が可能で、*ツール → スクリプトを実行 (.cs/.dll)...*（`tools.run_script`）または `--run-script` により `ScriptPortal.Vegas` 互換（クリーンルーム、プロトコル 6）の `.cs`/`.dll` スクリプトをオンデマンド実行できます。API 名は MAGIX 公開の *VEGAS Pro Scripting API Summary* およびマニュアル FAQ に準拠した独自実装で、プロジェクト、トラック（名前、Mute/Solo、選択、`Events`、`Effects`、`Envelopes`）、イベント（`Start`/`Length`/`End`、名前、Mute、Locked、選択、`FadeIn`/`FadeOut`、`Effects`、`Envelopes`、同種トラック間移動・削除）、エフェクトパラメーターとキーフレーム（`Effect.Keyframes`、`Effect.Parameters`、`Effect.Preset`、検出済み OFX/VST3 の ID 指定生成を含む）、マーカー、`Vegas.Cursor`、`Transport.CursorPosition`/`SelectionStart`/`SelectionLength`、演算子付き厳密ティック `Timecode`、プロジェクトメディア（`Project.MediaPool`、`Media`、`VideoStream`/`AudioStream`）、テイク（`Takes`、`ActiveTake`、`AddTake`、`Take.Offset`/`Name`/`MediaPath`）、イベント生成（`AddVideoEvent`/`AddAudioEvent` + `AddTake`）および新規メディア読み込み（`MediaPool.AddMedia` または `new Media(path)`、1 実行あたり最大 64 件）に対応。`Microsoft.WindowsDesktop.App` ランタイムのある Windows では STA スレッド上で本物の `System.Windows.Forms` を使用し、Linux・macOS および `WindowsDesktop` 非導入環境では `crates/neoveg_ui/src/script_forms.rs` の独自レイヤーでダイアログ・フォーム（`MessageBox`、`Form.ShowDialog`、ファイル選択）が動作します。
- **インターフェイス・カーソル・カスタマイズ・アクセシビリティ:** 高密度エディトリアルデザインと 7 テーマ（*表示 → テーマ* または *オプション → ユーザー設定...*、コントラスト比 7:1 以上の WCAG AAA ハイコントラスト ダーク/ライトを含む）、**22 種類のオリジナルベクター編集カーソル**（`cursors.rs`：左右/隣接トリム、タイムストレッチ、*Split Trim*、フェードオフセット、*Slip*、*Slide*、クロスフェードスリップ、*スクラブ*、回転、自由/X/Y 移動、エンベロープハンド、ズーム +/-、範囲選択ボックス、ベジェマスクのペン/頂点ツール）、`もっと見る (…)` メニュー付き 22 px カラー縦ガターのトラックヘッダー、イベントのダークカプセルラベルとカスタマイズ可能なイベントボタン（`パン/クロップ`、`fx`、`…`）、*マスター バス* の中央 dB スケール、アニメーション可能なインタラクティブ **ベジェマスク** 行・プリセット・FX チェーンピルを備えた **パン/クロップ**、完全な **2D/3D Track Motion**（ピボット、向き、2D シャドウ/グロー、3D 親子合成、Z ソート、*off-axis* ステレオスコピック 3D カメラ）、**CC1–CC4** 字幕デコードと **インタラクティブ 360° 正距円筒ビュー** を備えた **ビデオ プレビュー**、クラシック配置タイプ（*クラシック*、*中級*、*最近*、*モダン*）を切り替えられる **編集 → プロパティ → レイアウト**、**スプラッシュ画面** と **ようこそ画面**、ドッキング可能な **カラーグレーディング** パネル、2 カラム **名前を付けてレンダリング...**、123 種類のネイティブカラーベクターアイコン（`crates/neoveg_ui/src/icons.rs`）、マニュアル順のトランスポート/編集バー（`開始`/`終了`/`長さ` 表示）、マルチタブのフローティングドックおよびダイアログの永続ドッキング（`Preferences.docked_dialogs`）、`F11`/`Shift+F11`/`Ctrl+F11`、`.neoveglayout` レイアウト保存。
- **Neoveg Encoder（バッチエンコードキューと複数出力）:** ドッキング領域外の独立ウィンドウ（上部ツールバーの **Encoder** ボタン、*表示 → Neoveg Encoder* `Ctrl+Alt+M`、*名前を付けてレンダリング...* 内の **Encoder に送る**、*ファイル → Neoveg Encoder へ送る...* `Ctrl+Shift+M`）および CLI `--encode` が全 7 テーマと統合。ソースごとの並列複数出力を持つ永続キュー（`encoder/queue.json`）、サムネイルと残り時間付きライブ *エンコード* パネル、協調的一時停止と個別/全体停止、*プリセットブラウザー*（システム、カスタム `.neovegpreset`、グループ、エイリアス）、*監視フォルダー*（自動検知・上書き防止）、*書き出し設定* ウィンドウ（`In`/`Out` 範囲、縦横比ロック付き *Crop*、5 種類のソーススケーリング、*ソース/出力* プレビュー）、エンコード前エフェクト（カラー補正、クリーンルーム 1D/3D `.cube` LUT、画像/名前/SMPTE タイムコード重ね合わせ、タイム調整）、`logs/neoveg_encoder*.log` へのログ記録を備えます。
- **ArtCraft & Blender 連携（*ツール → 連携*）:** **PhotoCraft**、**VectorCraft**、**FilmCraft**、**EffectCraft**、**AniCraft**、**SoundCraft**、**Blender** の自動検出と手動パス設定（*ユーザー設定 → 外部アプリ*）。編集用コピーを *テイク* としてやり取りする *Round-Trip*（PhotoCraft 用 PNG、VectorCraft 用クリーン SVG、SoundCraft 用 `bext` `TimeReference` 付き BWF WAV — 保存時自動リロード対応）、起動中 ArtCraft アプリ向け HMAC-SHA256 認証付き TCP ループバック（`127.0.0.1`）クライアント、FilmCraft への OTIO 書き出し、`.anicraft`/`.ecproj`/`.blend` シーンのヘッドレス PNG 連番レンダリングとタイムライン配置、FPS/マーカー/パン・クロップ同期用 `.py` スクリプト生成、Blender カメラ（`.json`）の *Track Motion*/*パン/クロップ* キーフレーム取り込みに対応します。
- **対応フォーマット:**
  - **交換形式（純 Rust・クリーンルーム）:** OpenTimelineIO `.otio`（スキーマ 0.17）、Final Cut Pro XML `.fcpxml`、Final Cut Pro 7 XML `.xml`（`xmeml`）、AAF `.aaf`、EDL CMX 3600 `.edl`、および SubRip `.srt` / W3C WebVTT `.vtt` 字幕（読み込み・書き出し対応）。*ファイル → 交換形式と字幕* またはドラッグ＆ドロップで利用可能。準拠した仕様書の版は [`crates/neoveg_engine/README.md`](crates/neoveg_engine/README.md) に記載しています。
  - **純 Rust（クロスプラットフォーム）:**
    - *プロジェクト・シーケンス・動画:* `.neoveg`（および旧 `.nvp`）、単独 `.nvegseq` シーケンス（ID 再マッピング付き入出力）、`AVI`（`MJPEG`/`DIB RGB24/32`/`YUY2`/`UYVY`/`I420` + PCM/G.711 読み込み；`AVI MJPEG` と `非圧縮 RGB24 AVI` 書き出し）、`Y4M`、アニメーション `GIF`（入出力）、`MP4`/`MOV`/`MKV`/`WEBM` コンテナーデマルチプレクサ（`media/demux/`）、可逆 `FFV1 v3` デコーダー（`media/ffv1.rs`）、クロスプラットフォーム フォールバックデコーダー（`media/fallback.rs`）。
    - *静止画・連番画像:* `PNG`、`JPEG`、`BMP`、`TIFF`、`WebP`（静止画・アニメーション）、`GIF`、`TGA`/`ICB`/`VDA`/`VST`、`ICO`/`CUR`、`PNM`（`PBM`/`PGM`/`PPM`/`PAM`）、`Radiance HDR`、`DDS`、`Farbfeld`、`QOI`、`OpenEXR`（`.exr`）、`DPX`（`.dpx`）、`Kodak Cineon`（`.cin`）、`WBMP`、`XBM`、`XPM`、`Amiga IFF/ILBM`（`.iff`/`.ilbm`/`.lbm`）、さらに 10 形式（`PNG`、`JPEG`、`TIFF`、`TGA`、`BMP`、`WebP`、`PNM`、`QOI`、`DPX`、`EXR`）での静止画保存および連番書き出し。
    - *オーディオ・MIDI:* `WAV` / `BWF` / `RF64`（PCM、Float32、ITU-T G.711 µ-law/A-law、IMA/DVI ADPCM）、`AIFF`/`AIFC`（入出力）、`Sun/NeXT AU/SND`、`Apple CAF`、`Sony Wave64`（`.w64`）、`Creative VOC`、`Amiga IFF 8SVX`（8bit PCM・Fibonacci Delta）、`QOA`（`.qoa`）、`FLAC`（`claxon`）、`Ogg Vorbis`（`.ogg`、`.oga`、`lewton`）、`MP3`（`puremp3`）、および **Standard MIDI File**（`.mid`/`.midi`、フォーマット 0/1/2、`midi.rs` のクリーンルームパーサー、ポリフォニックフォールバックプレビューおよび VST3 インストゥルメントへのルーティング）。
  - **ネイティブ システムバックエンド（Windows・Linux・macOS）:**
    - *Windows (Media Foundation):* `MP4`、`MOV`、`MKV`、`WEBM`、`M4V`、`M4A`、`AAC`、`WMV`、`ASF`、`WMA`、`3GP`、`3G2`、`M2TS`、`MTS`、`TS`、`MPG`、`MPEG`、`MPE`、`VOB`、`MP2`、`MPA`、`M4B`、`ALAC` のストリーミング読み込み・デコード、`IMFSinkWriter` による `MP4 (H.264/HEVC + AAC)`、`MOV (H.264 + AAC)`、`3GP (H.264 + AAC)`、`WMV`、および音声 `M4A (AAC)`、`AAC (ADTS)`、`WMA`、`MP3` のネイティブ書き出し。
    - *Linux (VA-API):* 動的ロードされる `libva.so.2`/`libva-drm.so.2` によるハードウェアデコード（`media/vaapi.rs`、GStreamer/LGPL 不使用）、純 Rust フォールバックへ自動切替。
    - *macOS (VideoToolbox / AudioToolbox):* `VTDecompressionSession` による H.264/HEVC 動画デコードおよび `ExtAudioFile` による音声デコード（`media/videotoolbox.rs`）、純 Rust フォールバックへ自動切替。
  - **互換形式への変換（Windows）:** 直接開けないファイル（対象外拡張子や非 MJPEG の AVI など独自リーダー未対応コーデック）では、システムデコーダーを用いて AVI MJPEG + PCM（映像）または 24bit WAV（音声のみ）へ変換し同じ位置へ読み込む選択肢を提示します（*ファイル → 互換形式にメディアを変換...* からも実行可能）。保存先をダイアログで選び、完了まで `.part` として書き出され、AVI は OpenDML 拡張（4 GB 制限なし、最大約 250 GB；収める必要がある場合のみ警告付きで JPEG 品質を 90 から 50 まで調整）を採用し、無音での穴埋めや無断トリムは一切行いません（`media/convert.rs`）。（`ffmpeg`/`ffprobe` は `tools/gen_media_fixtures.py` による外部テストオラクルおよびフィクスチャ生成にのみ使用し、リンクや配布は一切行いません。）
- **異常終了レポート:** 各 UI セッションは `logs/sessoes/` にロック済みマーカーを保持します。クラッシュ・強制終了・電源断後の次回起動時に *Neoveg が前回正常終了しませんでした* ウィンドウが開き、発生状況（エラーと箇所、または予告なき終了）とセッション中の隔離エラー一覧を表示、自動保存プロジェクトの復元、レポートコピー、ログフォルダーを開く操作を提供します。複数インスタンスの同時起動はクラッシュ扱いにならず、別インスタンスやプラグインホストのパニックも混在しません。
- **プロジェクトのプロパティ:** VEGAS Pro 同様に *ビデオ*、*オーディオ*、*ルーラー*、*概要*、*オーディオ CD* の 5 タブを搭載。概要と UPC/EAN コードは書き出される `.cue` シートへ反映されます。
- **Windows と Linux:** 両 OS 向けに Rust 製ネイティブインストーラーとポータブルパッケージを提供。Linux ではアプリメニュー登録、`neoveg` コマンド、`.neoveg` ファイル関連付けに対応（*Linux 用インストーラー* 参照）。
- **省スペック環境への配慮:** プレビュー解像度（*ドラフト / プレビュー / 良好 / 最高*）、`.avi` プロキシ生成、選択領域のプリレンダリング、RAM/フレームキャッシュ上限設定、`-C target-cpu=native` を使わないポータブルビルド。

## 実行方法

```bash
# GUI を起動（プロジェクトやメディアを引数で指定可能）
cargo run -p neoveg
cargo run -p neoveg -- my_project.neoveg

# コマンドラインインターフェイス (CLI)
cargo run -p neoveg -- --list-commands
cargo run -p neoveg -- --probe-integrations
cargo run -p neoveg -- --command insert.marker my_project.neoveg updated.neoveg
cargo run -p neoveg -- --run-script my_script.cs
cargo run -p neoveg -- --render my_project.neoveg output.avi
cargo run -p neoveg -- --encode input.mp4 output.mp4 sys.web_1080p_hq
```

ポータブルビルドでは、ログ、自動保存、ユーザー設定（`preferencias.json`）、レイアウト（`.neoveglayout`）、プロキシ、プリレンダリングが実行ファイルと同じフォルダーに保存されます（読み取り専用フォルダーから起動した場合はシステムの一時フォルダーを使用）。

## 使い方

### はじめに

1. Neoveg（`cargo run -p neoveg` またはポータブル版の実行ファイル）を起動します。**ようこそ画面** で 5 つのアスペクト比（*Widescreen 16:9*、*Fullscreen 4:3*、*Portrait 9:16*、*Square 1:1*、*Scope 2.4:1*）から選び、解像度・フレームレート・*Drop-Frame* を設定するか、*プロジェクトを開く* から既存プロジェクトを開きます。この画面は *ヘルプ → ようこそ画面* からいつでも再度開けます。
2. *ファイル → メディアのインポート...*（`Ctrl+I`）またはウィンドウへのドラッグ＆ドロップで動画・音声・静止画・連番画像を読み込みます。クリップは *プロジェクト メディア* に表示され、タイムラインへのドラッグまたはダブルクリックでカーソル位置に挿入できます。カット前に *In*/*Out* 点を決めたい場合は **トリマー** パネル（`Alt+2`）を使用します。
3. *プロジェクトのプロパティ*（`Alt+Enter`）が手動変更されていない場合、最初に読み込んだメディアの形式に合わせてプロジェクトの解像度とフレームレートが自動設定されます。
4. Windows では直接デコード対象外のコーデックを開いた際、`AVI MJPEG + PCM` または `24bit WAV` への安全な変換オプションが自動で表示されます（*ファイル → 互換形式にメディアを変換...*）。
5. `Ctrl+S`（別名/バージョン保存は `Ctrl+Shift+S` / `Ctrl+Alt+S`）で `.neoveg` として保存します。予期しない終了時には自動復旧が機能します。

### タイムラインでの編集

| 操作 | ショートカット |
|---|---|
| 再生 / 一時停止 / 先頭から再生 / 停止 | `Space` / `Shift+Space` / `Esc` |
| 逆再生スクラブ / 一時停止 / 早送りスクラブ | `J`, `K`, `L` |
| 前のフレーム / 次のフレーム | `Alt+←` / `Alt+→` |
| 次 / 前の編集ツール | `D` / `Shift+D` |
| カーソル位置でイベントを分割 | `S` |
| カーソル位置まで開始 / 終了をトリム | `Alt+[` / `Alt+]` |
| 時間選択範囲にトリム | `Ctrl+T` |
| 選択開始 / 終了 / ループ設定 | `I` / `O` / `Q` (`Shift+Q`) |
| 次 / 前のテイク | `T` / `Shift+T` |
| グループ化 / 解除 / グループ選択 | `G` / `U` / `Shift+G` |
| マーカー / リージョン挿入 | `M` / `R` |
| トラックのボリューム / パン エンベロープ | `V` / `P` |
| 選択トラックのミュート / ソロ | `Z` / `X` |
| ビデオ / オーディオ / バストラック挿入 | `Ctrl+Shift+Q` / `Ctrl+Q` / `Ctrl+B` |
| スナップ（`snapping`）/ オートリップル | `F8` / `Ctrl+L` |
| 水平ズーム / 波形の垂直ズーム | `↑` / `↓` (`Shift+↑` / `Shift+↓`) |
| 元に戻す / やり直す | `Ctrl+Z` / `Ctrl+Y` |
| イベントのプロパティ | `F2` |
| Neoveg Encoder / タイムライン送信 | `Ctrl+Alt+M` / `Ctrl+Shift+M` |
| 名前を付けてレンダリング... | `Ctrl+M` |

- **編集ツールとモード（`D` / `Shift+D`）:** トランスポートバーや *編集 → 編集ツール* から、**通常**（*通常* または *スリップ* での移動・トリム）、**エンベロープ**（エンベロープ点のみ移動）、**選択**（矩形選択、`Shift`/`Ctrl` で追加）、**ズーム**（矩形拡大、単クリックで全体表示）を切り替えられます。
- **フェード・ゲイン・キーフレーム:** 同じトラック上でイベントを重ねると自動でクロスフェードが作成されます。上隅のハンドルでフェード、オーディオクリップ上部のバーで dB ゲインを調整でき、トラック下の行で *Track Motion* とトラック FX のキーフレームを編集できます。
- **ヘッダー・トランスポート・表形式編集:** 下部バーは VEGAS Pro 順で `開始`/`終了`/`長さ` の確認・入力、`Playback Rate`（`0.25×`–`4.00×`）制御を備えます。トラックヘッダーの `もっと見る`（`…`）ボタンで表示ボタンを設定でき、**編集の詳細** パネル（`Alt+3`）で表形式の編集が可能です。
- **シーケンス・単独 `.nvegseq`・ネスト:** タイムライン上部のタブで同一プロジェクト内の複数シーケンスを切り替えられ、単独 `.nvegseq` シーケンスの読み込み・書き出し（*編集 → シーケンス → シーケンスのインポート/エクスポート...* またはドラッグ＆ドロップ）および未保存のネスト済み `.neoveg` タブの自動復旧（`autosave/recuperacao_aninhados.json`）に対応します。多階層ネスト（孫プロジェクト含む）のメモリ上リアルタイム伝播もサポート。*プロジェクト メディア* からシーケンスをタイムラインへドラッグするか *選択範囲を新規シーケンスにネスト*（`sequence.nest_selection`）を使用でき、循環参照の厳格な防止と最大 32 階層の上限が適用されます。

### テイク

1 つのイベントに複数の代替メディア（テイク）を保持できます。*プロジェクト メディア* でメディアを右クリックし *選択イベントにテイクとして追加* を選択し、`T` / `Shift+T` またはイベントの *テイク* サブメニューでアクティブテイクを切り替えます。*オプション → すべてのテイクをスリップ* を有効にすると、スリップ時にすべてのテイクが同期します。

### エフェクト・ジェネレーター・カラーグレーディング・ビデオスコープ

- **ビデオ FX・オーディオ FX・トランジション:** 8 カテゴリー（カラー、ライト、ぼかしとシャープ、スタイライズ、ディストート、コンポジットと枠、スタビライズ、クロップ）のネイティブビデオエフェクト 46 種とトランジション 33 種を搭載。カテゴリーや検索で絞り込み、プリセットサムネイルをイベント・トラック・トランジションへドラッグするか、*イベント*・*トラック*・*マスター* ボタンで適用します。`★` でお気に入り登録できます。
- **メディア ジェネレーター:** *メディア ジェネレーター* タブから、輪郭・影付きアニメーションテキスト、単色、グラデーション、チェッカーボード、カラーバー、ノイズ、**テストパターン**（コンバージェンスグリッド、グレーランプ、ゾーンプレート、ピクセルグリッド）、フラクタル **ノイズテクスチャ**、およびサードパーティ **OFX** ジェネレーター（*サードパーティ*）を挿入できます（*カーソル位置に挿入* またはドラッグ）。
- **カラーグレーディングとビデオスコープ:** *カラーグレーディング* を開く（または *ビデオ プレビュー* ヘッダーの **カラー** をクリック）と、`FxKind::ColorCorrector` と連動するカラーホイール（*Lift*、*Gamma*、*Gain*）、*色温度 × 色合い* パッド、彩度、プリセットを調整できます。**ビデオスコープ** タブ（*RGB + Luma ヒストグラム*、*波形*、*ベクトルスコープ*、*RGB パレード*）で信号をリアルタイム監視できます。
- **外部プラグイン（OFX・VST3）:** *プラグイン マネージャー*（`Ctrl+Alt+1`）で VST3・OFX の検索パスを管理します。すべてのプラグインは隔離サブプロセス `--plugin-host` 内で動作し、OFX のタイル処理・補助入力付き *General* コンテキスト・ジェネレーター、および VST3 のネイティブ GUI（`IPlugView`）・リアルタイムサンプル精度オートメーション・MIDI インストゥルメント（`.mid`/`.midi`）・状態保存（`IComponent::getState`/`setState`）・自動レイテンシ補正（PDC）を安全に利用できます。

### オーディオ・ミキサー・5.1 サラウンド・ラウドネス

- **ミキサーと補助バス（`Bus A`..`Bus Z`）:** *ミキサー* の **＋ バス** または *挿入 → オーディオ バス* で最大 26 バスを作成し、*pre/post-fader* センドやタイムライン表示（`Ctrl+B`）を行えます。
- **マスター バス:** 右側パネルに `fx`、`Mono`、`Dim`（`-20 dB`）、`M`（ミュート）を備えたステレオモニターを配置。
- **5.1 サラウンド パンナー:** *サラウンド パンナー* タブ（または *プロジェクトのプロパティ*）で *マスターモード* を `5.1 Surround` に切り替え、各トラックやバスを ITU-R BS.775 リング上に配置し、LFE を調整して 6 チャンネルマルチチャンネル WAV としてレンダリングできます。
- **EBU R128 ラウドネス メーター（`Ctrl+Alt+7`）:** `M`、`S`、`I`、`LRA`、`True Peak` を設定可能な目標値（*EBU R128 -23 LUFS*、ストリーミング、ATSC、ポッドキャスト）で確認でき、オフライン解析（*プロジェクトを解析* / *選択範囲を解析*）も実行できます。

### レンダリング

*ファイル → 名前を付けてレンダリング...*（`Ctrl+M`）は、検索・★ お気に入り・範囲サマリー（*プロジェクト全体* または *ループ領域*）と **対応 31 出力フォーマット** を備えた 2 カラムウィンドウを開きます：`MP4 (H.264/HEVC + AAC)`、`MOV`、`3GP`、`WMV`（Windows Media Foundation）、`AVI MJPEG` と `非圧縮 RGB24 AVI`（OpenDML 対応）、アニメーション `GIF`、`Y4M`、10 種の連番画像（`PNG`、`JPEG`、`TIFF`、`TGA`、`BMP`、`WebP`、`PNM`、`QOI`、`DPX`、`EXR`）、およびオーディオ（`WAV`、`BWF`、`RF64`、`Wave64`、`AIFF`、`AU`、`CAF`、`QOA`、`M4A`、`AAC`、`WMA`、`MP3`）。同じ画面から **Encoder に送る** を押すか、`neoveg --render project.neoveg output.ext` でヘッドレス書き出しも可能です。

### Neoveg Encoder（バッチキューと監視フォルダー）

**Neoveg Encoder** は上部バーの **Encoder** ボタン、*表示 → Neoveg Encoder*（`Ctrl+Alt+M`）、*ツール → Neoveg Encoder*、*名前を付けてレンダリング...* 内の **Encoder に送る**、または *ファイル → Neoveg Encoder へ送る...*（`Ctrl+Shift+M`、現在のタイムラインまたは選択範囲をキューへ追加）から開けます。タブバーの **分離** をクリックすると、ドック外の広い独立ウィンドウとして利用できます。

- **4 つのパネル:** *エンコードキュー*（1 ソースへの複数並列出力、複製、並べ替え、フッテージ解釈）、*エンコード（ライブ）*（サムネイル、残り時間、協調的一時停止、個別/全体停止）、*プリセットブラウザー*（システム、カスタム `.neovegpreset`、複数出力グループ）、*監視フォルダー*（入力フォルダーの自動検知）。
- **書き出し設定:** `In`/`Out` 範囲トリム、縦横比ロック付き *Crop*、5 種類のスケーリングモード、カラー補正、1D/3D `.cube` LUT、画像/名前/SMPTE タイムコード重ね合わせ、*タイム調整*（%）。コマンドラインからも利用できます：`neoveg --encode input.mp4 output.mp4 sys.web_1080p_hq`。

### 交換形式と字幕

*ファイル → 交換形式と字幕*（またはファイルの本ウィンドウへのドラッグ＆ドロップ）により、**OpenTimelineIO（`.otio`）**、**Final Cut Pro XML（`.fcpxml`）**、**Final Cut Pro 7 XML（`.xml`、`xmeml`）**、**AAF（`.aaf`）**、**EDL CMX 3600（`.edl`）** のタイムラインおよび **SubRip（`.srt`）** / **WebVTT（`.vtt`）** 字幕を読み込み・書き出しできます。

### 外部連携（ArtCraft & Blender）

*ツール → 連携 (ArtCraft と Blender)...*（手動パスは *オプション → ユーザー設定... → 外部アプリ*）から、**PhotoCraft**、**VectorCraft**、**FilmCraft**、**EffectCraft**、**AniCraft**、**SoundCraft**、**Blender** と連携できます。*テイク* としての *Round-Trip* コピー作成（PNG、クリーン SVG、保存時自動再読込付き WAV/BWF）、FilmCraft 向け OTIO 書き出し、`.anicraft`/`.ecproj`/`.blend` シーンの PNG 連番レンダリングとタイムライン挿入、同期用 `.py` スクリプト生成、Blender JSON カメラの取り込みが可能です。

### C# スクリプト（`.cs`/`.dll`）

*ツール → スクリプトを実行 (.cs/.dll)...*（または `neoveg --run-script my_script.cs`）により、`ScriptPortal.Vegas` API 互換の `.NET` スクリプトを単一の「元に戻す」単位で実行します。Windows（`Microsoft.WindowsDesktop.App` 導入時）では `System.Windows.Forms` を、Linux・macOS および `WindowsDesktop` 非導入環境では `script_forms.rs` の独自レイヤーを使用します。

### レイアウト・外観・ショートカット

- **レイアウトと分離ウィンドウ:** *編集 → プロパティ → レイアウト* で *クラシック*・*中級*・*最新*（3 カラム）やドック位置・タブ配置を切り替えられます。すべてのタブ、*ビデオ プレビュー*、*マスター バス* はドラッグで並べ替えたりネイティブウィンドウとして分離でき、`.neoveglayout` に保存可能です。
- **テーマとショートカット:** *表示 → テーマ* で 7 テーマ（AAA ハイコントラスト 2 種を含む）を切り替え、単一コマンドテーブルのすべてのコマンドを *ヘルプ → キーボード ショートカット* または *オプション → ユーザー設定...* でカスタマイズできます。

## ポータブルパッケージ化と品質ゲート

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python tools/check_licenses.py
python build_portable.py
```

GitHub CI（`.github/workflows/ci.yml`）は、すべてのプルリクエストに対して Windows と Linux（Ubuntu）の両方でこれらのゲートを実行します（テストは 1 件ずつ順番に実行され、メディアフィクスチャはその場で生成されます）。

`build_portable.py` はリリースビルドを行い、インストーラー・プロプライエタリフォント・外部バイナリに依存しないポータブルパッケージをリポジトリ直下の `dist/` に生成します（オープンフォント `assets/fonts/`（`OFL-1.1`）と `i18n/*.json` を同梱）。コピーと `SHA-256` 検証の完了後に cargo ビルドディレクトリ（`target/` または `CARGO_TARGET_DIR`）を自動削除し、ビルド・検証・コピーのいずれかが失敗した場合はディレクトリを残してエラーを報告します。`--keep-target` でビルドディレクトリを保持、`--out` で出力先を変更できます。`dist/` と `target/` は `.gitignore` 対象です。

### Windows 用インストーラー

```bash
python build_installer.py
```

`build_installer.py` は `build_portable.py` でポータブルパッケージを作成し、`dist/` を ZIP 圧縮して Rust 製ネイティブインストーラー `neoveg-setup.exe`（`crates/neoveg/src/setup/`、`MIT OR Apache-2.0`、サードパーティ製ツール不使用）の末尾に結合します。出力先は `dist-installer/Neoveg-Setup-<バージョン>.exe`（同じく `.gitignore` 対象）で、`--no-build` と `--keep-target` オプションにも対応します。

インストーラーはインストール対象を選択できます：

- **自分のみ:** `%LOCALAPPDATA%\Programs\Neoveg`（管理者権限不要）。
- **すべてのユーザー:** `%ProgramFiles%\Neoveg`（UAC 確認あり）。

スタートメニューおよび（チェック時に）デスクトップへショートカットを作成し、`.neoveg` ファイルの関連付けも行えます。*設定 → アプリ*（プログラムの追加と削除）にアンインストーラー `desinstalar.exe` とともに登録されます。

インストール版では、設定・レイアウト・ログ・復旧データはプログラムフォルダーではなく `%APPDATA%\Neoveg` に保存されます。データフォルダーはアンインストーラーで削除オプションを選んだ場合のみ削除されます。ポータブル版は引き続き実行ファイルと同じ場所にすべて保存します。

サイレントモード：`Neoveg-Setup.exe --silent [--user | --all-users] [--dir <フォルダー>] [--desktop] [--no-assoc]`。ウィンドウなしのアンインストール：`desinstalar.exe --uninstall --silent [--remove-data]`。

### Linux 用インストーラー

```bash
python3 build_installer.py
```

Linux では同じ `build_installer.py` が `dist-installer/` に 2 つのファイルを生成します：

- `Neoveg-Setup-<バージョン>-linux-x86_64`: 同じウィンドウとオプションを備えた Rust 製インストーラー。実行権限（`chmod +x`）を付けて起動します。
- `Neoveg-<バージョン>-linux-x86_64.tar.gz`: ポータブルパッケージ。展開して `./neoveg` を実行すると、データは実行ファイルと同じ場所に保存されます。

インストーラーはインストール対象を選択できます：

- **自分のみ:** `~/.local/opt/neoveg`（パスワード不要）。デスクトップエントリは `~/.local/share/applications`、`neoveg` コマンドは `~/.local/bin` に配置されます。
- **すべてのユーザー:** `/opt/neoveg`（`pkexec` で管理者パスワード入力）。デスクトップエントリと MIME タイプは `/usr/local/share`、コマンドは `/usr/local/bin` に配置されます。`pkexec` がない場合は `sudo ./Neoveg-Setup-<バージョン>-linux-x86_64 --silent --all-users` を実行してください。

ショートカットは freedesktop.org の *Desktop Entry Specification* に準拠し、対応デスクトップ環境（GNOME、KDE Plasma、Xfce 等）のアプリメニューに Neoveg アイコン付きで表示されます。選択時はデスクトップにも配置されます（自分のみインストール時、`~/.config/user-dirs.dirs` から取得）。`.neoveg` の関連付けは `application/x-neoveg-project`（*Shared MIME-info*）を登録します。`update-desktop-database`、`update-mime-database`、`xdg-mime`、`gio` が存在すれば実行し、なくてもインストールは続行されます。アンインストールはショートカットの右クリックメニュー *Neoveg のアンインストール*、またはインストール先での `desinstalar --uninstall` から行えます。

インストール版の保存先は `$XDG_DATA_HOME/Neoveg`（通常 `~/.local/share/Neoveg`）です。サイレントモードは Windows と同じオプションに対応します：`./Neoveg-Setup-<バージョン>-linux-x86_64 --silent [--user | --all-users] [--dir <フォルダー>] [--desktop] [--no-assoc]` および `desinstalar --uninstall --silent [--remove-data]`。

実行ファイルはビルド環境の glibc に依存するため、より古いディストリビューションで動かす場合は対象の中で最も古い環境でコンパイルしてください。

### 言語（`i18n/*.json`）

Neoveg は `i18n/` 内の JSON カタログにより **ブラジルポルトガル語（`pt-BR`、デフォルト）**、**英語（`en`）**、**スペイン語（`es`）**、**フランス語（`fr`）**、**ドイツ語（`de`）**、**日本語（`ja`）** に対応しています。言語は **ユーザー設定 → 一般**、**ようこそ画面**、または起動時の `--lang <コード>` / `NEOVEG_LANG` でリアルタイムに切り替えられます。日本語では OS にインストールされたフォントから CJK グリフを実行時に読み込みます。UI は VEGAS Pro と同様にシステムフォントを使用します（Windows では Segoe UI・代替 Tahoma、Linux では Noto Sans・Liberation Sans・DejaVu Sans、等幅欄は Consolas または Cascadia Mono。システムのフォントフォルダーから直接読み込み、再配布しません。**ユーザー設定 → 一般** から内蔵フォントへ戻せます）。文字・ボタン・パネルの大きさは **表示 → インターフェイスの倍率**（拡大・縮小・100%、50%〜300% を 10% 刻み）またはユーザー設定の *インターフェイスの倍率* で変更できます（既定ショートカットはなく、*キーボード ショートカット* から設定可能）。*タイム ディスプレイ* にはシステム標準サンセリフ体の太字（Segoe UI Bold）と等幅数字を使用します。

## パリティ進捗

- [ロードマップ](ROADMAP.md)
- [VEGAS Pro パリティマトリクス](docs_obsidian/ui_references/matriz_de_paridade.md)
- [アセットの帰属表示とライセンス](ATTRIBUTION.md)

## ライセンス

Neoveg は次のいずれかのライセンスを選択して利用できるデュアルライセンスで配布されています：

- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

明示的な別段の記載がない限り、本プロジェクトへの収録を目的として提出されたすべてのコントリビューションは、追加条件なしに上記のデュアルライセンスの下で提供されるものとします。
