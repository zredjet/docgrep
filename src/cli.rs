//! Command line definition. All user-facing help text is Japanese.

use std::path::PathBuf;

use clap::error::{ContextKind, ErrorKind};
use clap::{ArgAction, Parser, ValueEnum};

use crate::model::Part;

const HELP_TEMPLATE: &str = "\
{about}

使い方: {usage}

{all-args}{after-help}";

const AFTER_HELP: &str = "\
例:
  docgrep サーバ 仕様書.docx             「サーバ」を含む箇所（「サーバー」も部分一致でヒット）
  docgrep -e 'サーバ(?!ー)' docs/        「サーバー」を除外して検索
  docgrep -i ｓｅｒｖｅｒ docs/          全角英字の大文字小文字だけ同一視（半角 server は別）
  docgrep --parts note,comment 要確認 .  脚注・文末脚注とコメントだけを検索
  docgrep -c サーバ docs/                ファイルごとの件数
  docgrep -- -foo .                      `-` で始まる語は `--` の後に書く

--parts の値（カンマ区切りで複数指定可）:
  body 本文 / table 表 / textbox テキストボックス / note 脚注・文末脚注 / comment コメント /
  header ヘッダー・フッター / toc 目次 / all すべて（Excel には影響しません）

補足:
  - 対象: .docx .docm .dotx .dotm / .xlsx .xlsm .xltx .xltm .xlsb .xls .ods
  - 表記ゆれ（全角半角・かなカナ・長音・Unicode 正規化・空白）は一切吸収しません
  - grep と違い -e は PATTERN を取らず、-C は文字数、-P は段落全文表示です
  - -l / -c / --json は同時に指定できません
  - ページ p.12 は Word 保存時のレイアウト情報による推定、p.12+ は明示改ページだけから
    数えた参考値（そのページ以降）です。-- はページのない箇所（ヘッダー・フッターなど）

終了コード: 0 = ヒットあり、1 = ヒットなし、2 = エラーあり";

#[derive(Debug, Parser)]
#[command(
    name = "docgrep",
    // Same usage line on every OS (not "docgrep.exe" on Windows).
    bin_name = "docgrep",
    version,
    about = "Word / Excel ファイルの中身を厳密一致で検索します（表記ゆれの吸収はしません）",
    help_template = HELP_TEMPLATE,
    after_help = AFTER_HELP,
    disable_help_flag = true,
    disable_version_flag = true,
    next_help_heading = "オプション"
)]
pub struct Cli {
    /// 検索する文字列（`-` で始まる場合は `--` の後に書く）
    #[arg(value_name = "PATTERN", help_heading = "引数", display_order = 0)]
    pub pattern: String,

    /// 検索するファイルまたはディレクトリ（省略時はカレントディレクトリ）
    #[arg(value_name = "PATH", help_heading = "引数", display_order = 1)]
    pub paths: Vec<PathBuf>,

    /// 大文字小文字を区別しない（全角半角・かなカナは区別したまま）
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// PATTERN を正規表現として扱う（fancy-regex 構文、先読み・後読み可）
    #[arg(short = 'e', long)]
    pub regex: bool,

    /// マッチの前後に表示する文字数（行数ではない。既定: 30）
    #[arg(
        short = 'C',
        long,
        value_name = "N",
        default_value_t = 30,
        hide_default_value = true
    )]
    pub context: usize,

    /// 前後を切り詰めず、段落（Excel はセル）の全文を表示する
    #[arg(short = 'P', long)]
    pub paragraph: bool,

    /// Word の検索対象パート（カンマ区切り、既定: all。値は下記参照）
    #[arg(
        long,
        value_name = "LIST",
        default_value = "all",
        hide_default_value = true,
        value_parser = parse_parts
    )]
    pub parts: PartSet,

    /// ヒットしたファイルのパスだけを表示する
    #[arg(short = 'l', long, conflicts_with_all = ["count", "json"])]
    pub files_with_matches: bool,

    /// ファイルごとのヒット数だけを「パス:件数」で表示する
    #[arg(short = 'c', long, conflicts_with = "json")]
    pub count: bool,

    /// JSON Lines（1マッチ1行）で出力する
    #[arg(long)]
    pub json: bool,

    /// 色付け: auto（端末への出力で NO_COLOR 未設定のとき）/ always / never（既定: auto）
    #[arg(
        long,
        value_enum,
        value_name = "WHEN",
        default_value_t = ColorWhen::Auto,
        hide_default_value = true,
        hide_possible_values = true
    )]
    pub color: ColorWhen,

    /// ディレクトリ探索の深さの上限（1 = 指定ディレクトリの直下だけ）
    #[arg(long, value_name = "N")]
    pub max_depth: Option<usize>,

    /// 並列数（既定: CPU 数。0 も既定と同じ）
    #[arg(short = 'j', long, value_name = "N")]
    pub threads: Option<usize>,

    /// ヘルプを表示する
    #[arg(short = 'h', long, action = ArgAction::Help)]
    help: Option<bool>,

    /// バージョンを表示する
    #[arg(short = 'V', long, action = ArgAction::Version)]
    version: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

/// Word parts selected by `--parts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartSet {
    pub body: bool,
    pub table: bool,
    pub textbox: bool,
    pub note: bool,
    pub comment: bool,
    pub header: bool,
    pub toc: bool,
}

impl PartSet {
    pub const ALL: PartSet = PartSet {
        body: true,
        table: true,
        textbox: true,
        note: true,
        comment: true,
        header: true,
        toc: true,
    };

    /// Whether units of `part` are searched. Excel cells are always searched.
    pub fn allows(&self, part: &Part) -> bool {
        match part {
            Part::Body => self.body,
            Part::Table { .. } => self.table,
            Part::TextBox => self.textbox,
            Part::Toc => self.toc,
            Part::Footnote { .. } | Part::Endnote { .. } => self.note,
            Part::Comment { .. } => self.comment,
            Part::Header | Part::Footer => self.header,
            Part::Cell => true,
        }
    }

    const NONE: PartSet = PartSet {
        body: false,
        table: false,
        textbox: false,
        note: false,
        comment: false,
        header: false,
        toc: false,
    };
}

/// Japanese message for a command line parse error (clap's own messages are English).
pub fn error_message(e: &clap::Error) -> String {
    let get = |kind| e.get(kind).map(|v| v.to_string()).unwrap_or_default();
    let arg = get(ContextKind::InvalidArg);
    let value = get(ContextKind::InvalidValue);
    let main = match e.kind() {
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            let mut msg = format!("{arg} の値「{value}」が不正です");
            if let Some(source) = std::error::Error::source(e) {
                msg.push_str(&format!(": {source}"));
            }
            let valid = get(ContextKind::ValidValue);
            if !valid.is_empty() {
                msg.push_str(&format!("（指定できる値: {valid}）"));
            }
            msg
        }
        ErrorKind::UnknownArgument => format!(
            "不明なオプション「{arg}」です。`-` で始まる検索語は `--` の後に書いてください（例: docgrep -- -foo .）"
        ),
        ErrorKind::MissingRequiredArgument => format!("必須の引数 {arg} がありません"),
        ErrorKind::ArgumentConflict => {
            let prior = get(ContextKind::PriorArg);
            if prior.is_empty() || prior == arg {
                format!("{arg} は1回だけ指定できます")
            } else {
                format!("{arg} と {prior} は同時に指定できません")
            }
        }
        _ => {
            let rendered = e.render().to_string();
            let first = rendered.lines().next().unwrap_or_default();
            let first = first.strip_prefix("error: ").unwrap_or(first);
            format!("コマンドライン引数が不正です: {first}")
        }
    };
    format!("{main}\n詳しくは docgrep --help を参照してください。")
}

fn parse_parts(value: &str) -> Result<PartSet, String> {
    let mut set = PartSet::NONE;
    for name in value.split(',') {
        match name.trim() {
            "all" => set = PartSet::ALL,
            "body" => set.body = true,
            "table" => set.table = true,
            "textbox" => set.textbox = true,
            "note" => set.note = true,
            "comment" => set.comment = true,
            "header" => set.header = true,
            "toc" => set.toc = true,
            "" => return Err("空の項目があります".to_string()),
            other => {
                return Err(format!(
                    "不明なパート「{other}」です（body, table, textbox, note, comment, header, toc, all のいずれか）"
                ));
            }
        }
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parts_are_parsed() {
        let set = parse_parts("body,note").unwrap();
        assert!(set.body && set.note && !set.table && !set.header);
        assert_eq!(parse_parts("all").unwrap(), PartSet::ALL);
        assert!(parse_parts("body,xyz").is_err());
        assert!(parse_parts("body,").is_err());
    }

    #[test]
    fn output_modes_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["docgrep", "-l", "-c", "x"]).is_err());
        assert!(Cli::try_parse_from(["docgrep", "-l", "--json", "x"]).is_err());
        assert!(Cli::try_parse_from(["docgrep", "-c", "--json", "x"]).is_err());
        assert!(Cli::try_parse_from(["docgrep", "-l", "x"]).is_ok());
    }

    #[test]
    fn parse_errors_are_japanese() {
        let msg = |args: &[&str]| error_message(&Cli::try_parse_from(args).unwrap_err());
        assert!(msg(&["docgrep"]).contains("必須の引数 <PATTERN> がありません"));
        assert!(msg(&["docgrep", "-x", "a"]).contains("不明なオプション「-x」です"));
        assert!(msg(&["docgrep", "-l", "-c", "a"]).contains("同時に指定できません"));
        assert!(msg(&["docgrep", "--parts", "foo", "a"]).contains("不明なパート「foo」です"));
        assert!(
            msg(&["docgrep", "--color", "red", "a"]).contains("指定できる値: auto, always, never")
        );
        assert!(msg(&["docgrep", "-C", "x", "a"]).contains("--context <N> の値「x」が不正です"));
    }

    #[test]
    fn hyphen_pattern_after_double_dash() {
        let cli = Cli::try_parse_from(["docgrep", "--", "-foo", "."]).unwrap();
        assert_eq!(cli.pattern, "-foo");
        assert_eq!(cli.paths, vec![PathBuf::from(".")]);
    }
}
