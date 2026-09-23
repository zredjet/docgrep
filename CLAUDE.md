# docgrep

Word / Excel ファイルの中身をコンソールから **厳密一致** で検索し、
ページ（推定）・章番号＋見出し・前後の文脈・キーワードをカラー表示する Rust 製 CLI。
対象 OS: Windows 10/11、macOS（Apple Silicon / Intel）。単一バイナリで動き、外部ソフトに依存しない。

詳細仕様は `docs/SPEC.md`。**作業前に、そのフェーズに関係する節を必ず読むこと。**

## 最重要ルール

- 検索語・本文ともに **一切の正規化をしない**（全角半角、ひらがな/カタカナ、長音、NFC/NFD、空白）。
  大文字小文字も `-i` 指定時以外は区別する。Word のあいまい検索を避けることがこのツールの存在理由。
- 仕様の正は `docs/SPEC.md`。仕様にない判断をしたら SPEC.md §14「決定ログ」に1行追記する。
- 仕様が曖昧・矛盾している、または実データで仕様の前提が崩れたら、推測で進めずにユーザーに質問する。
- 壊れた/想定外のファイルで panic しない。ファイル単位でエラーを報告して処理を続行する。
  本番コードで `unwrap()` / `expect()` / 範囲外になりうるインデックスアクセスは禁止（テストコードは可）。
- 外部コマンド（Word、LibreOffice、pandoc 等）を呼ばない。純 Rust で完結させる。
- ユーザー向けメッセージと `--help` は日本語。コード・識別子・コメント・コミットメッセージは英語。
- クレートは `cargo add` で最新安定版を入れる。API は記憶で書かず docs.rs で確認してから使う。
- 依存を追加・更新したら `cargo about generate --locked about.hbs -o THIRD_PARTY_LICENSES.md` で
  サードパーティライセンス一覧を再生成してコミットする。`about.toml` の許可リストに無いライセンスは、勝手に追加せずユーザーに確認する。

## コマンド

- ビルド: `cargo build`
- テスト: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings`
- 整形: `cargo fmt`
- 手動確認: `cargo run -- <PATTERN> <PATH>...`

フェーズ完了時は `cargo fmt --check`、clippy、test がすべて通っていること。

## ソース構成

```
src/
  main.rs          エントリポイント、終了コード（lib の薄いラッパー）
  lib.rs           run(): walk → 抽出 → 検索（rayon で並列、出力は引数順に並べ直す）、サマリ、終了コード
  error.rs         FileError（ファイル単位の警告/エラー）
  cli.rs           clap 定義（help は日本語）
  walk.rs          パス展開（Windows 用 glob）、ディレクトリ再帰、スキップ規則
  model.rs         TextUnit / Location / Part / Heading / Match
  matcher.rs       リテラル・正規表現・-i。char オフセットで Match を返す
  word/
    mod.rs         docx 全体の流れ（パッケージを開く → 各パートを抽出）
    package.rs     zip、.rels 解決、暗号化判定
    xml.rs         名前空間・属性・on/off の共通処理、補助パート用の要素ウォーカー
    styles.rs      スタイルチェーン解決（outlineLvl、numPr）
    numbering.rs   numbering.xml、カウンタ、numFmt 書式化
    headings.rs    本文ストリームの見出し判定と章番号の追跡（HeadingTracker）
    pages.rs       ページ推定（rendered / explicit の両モードを同時に追跡、表の行モデル）
    extract.rs     ストリーミング抽出 → TextUnit 列
  excel.rs         calamine で読み TextUnit 列にする
  output/
    mod.rs         文脈ウィンドウ、表示エントリのまとめ、char 単位スライス
    pretty.rs      カラー表示（anstream 経由）
    json.rs        JSON Lines
tests/
  common/mod.rs    フィクスチャ生成（DocxBuilder、xlsx 生成）
  fixtures/real/   ユーザーが Word 実機で保存したファイル + expected.toml
```

原則: 抽出（ファイル → TextUnit 列）・検索（TextUnit → Match）・表示 を分離する。抽出はパターンを知らない。

## ハマりどころ（必ず守る）

- Word は1語を複数の `w:r` に分割して保存する。検索は必ず段落テキストを結合してから行う。
- 位置は char 単位で扱う。正規表現のバイト位置は char 位置に変換し、スライスは char 境界で行う。
- XML 要素は「名前空間 URI + ローカル名」で判定する。接頭辞 `w:` に依存しない。Transitional / Strict 両対応。
- `w:pPr` / `w:rPr` / `w:tblPr` / `w:sectPr` などプロパティ要素の配下はテキスト抽出の状態に影響させない
  （例: `w:pPr/w:tabs/w:tab` はタブ文字ではない、`w:rPr/w:del` は削除範囲ではない）。
- `mc:AlternateContent` は `mc:Choice` だけ処理し `mc:Fallback` は無視する（テキストボックスが二重になるため）。
- テキストボックス（`w:txbxContent`）は段落の中に入れ子で現れる。段落スタックで処理し、外側の段落に混ぜない。
- フィールドの状態（`w:fldChar` begin/separate/end）は段落をまたいで持ち越す。入れ子にも対応する。
- styleId は言語依存（日本語版 Word では "1" などになる）。見出し判定に styleId を使わない。
- calamine の Range は開始位置オフセットを持つ。セル番地は `range.start()` を加算して算出する。
- 色は anstream / anstyle 経由でのみ出力する（`println!` に ANSI を直書きしない）。基本16色のみ使う。

## テスト

- フィクスチャは `tests/common` の生成関数で作る（XML 断片 → zip）。xlsx は rust_xlsxwriter で生成。
- `tests/fixtures/real/` の実ファイルと `expected.toml` はユーザー提供。存在すれば照合、無ければスキップ。
- 新機能・バグ修正には必ずテストを追加する。実ファイルで見つかった不具合は、最小 XML で再現するテストを先に書く。
- 出力の見た目は insta のスナップショットで固定する（色あり/なし両方）。

## 進め方

- フェーズは SPEC.md §12 の順に進める。1フェーズ = 1コミット以上。
- フェーズ完了時: SPEC.md §12 のチェックボックスを更新し、ユーザーが試せる確認コマンド例を提示する。
- 実ファイルを持っていないと検証できない事項（SPEC.md §14 の V 番号）は、自己判断で確定させずユーザーに確認を依頼する。
