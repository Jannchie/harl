//! shell 命令 → 用到的程序（`cd x && sudo env A=1 npm test | tee log` → npm、tee）。
//!
//! 算法移植自 dsh-context（Apache-2.0）的命令卡片，按 harl 的工具形状重写：
//! - 先按 shell 语法切段：认引号、heredoc、`$()` / 括号深度、注释；分隔符是 `;` `&&` `||` `|` `&` 换行；
//!   PowerShell 另认反引号转义、here-string（`@" … "@`）和块注释 `<# #>`。
//! - 每段剥掉前缀再取第一个词：重定向、`cd X &&`、export / `VAR=val` 赋值（记下来，`$VAR` 当命令时能解析）、
//!   包装命令（env、sudo、time、nohup、nice、timeout、stdbuf……连同它们带参数的选项）。
//! - 第一个词去掉路径和 .exe / .cmd / .bat；shell 关键字、函数定义、子 shell 记作 `shell`（PowerShell 是 `pwsh`）。
//! - 一条命令的程序列表 = 整条命令的主程序 + 其余各段的程序；后面跟着 `&&` / `;` / 换行的 `cd` 不算。
//!
//! 全程按字节处理：分隔符都是 ASCII，多字节字符原样搬进结果，切片不会落在字符中间出错。

use std::collections::HashMap;

use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect {
    Bash,
    Pwsh,
}

/// 赋值记下的变量：值拿不准（含展开、复合赋值）时是 None
type Vars = HashMap<String, Option<String>>;

/// 命令太长时只看开头这么多字节
const MAX_CMD: usize = 4096;

/// 包装命令：真正的程序在它们后面
const WRAPPERS: &[&str] =
    &["env", "sudo", "doas", "command", "exec", "time", "nohup", "nice", "stdbuf", "timeout", "caffeinate", "noglob", "nocorrect"];

/// 包装命令里带一个参数的选项（`sudo -u root`）
fn takes_arg(wrapper: &str, flag: &str) -> bool {
    let list: &[&str] = match wrapper {
        "env" => &["-u", "--unset", "-C", "--chdir", "-S", "--split-string"],
        "sudo" => &[
            "-u", "--user", "-g", "--group", "-p", "--prompt", "-C", "--close-from", "-D", "--chdir", "-h", "--host", "-r",
            "--role", "-t", "--type", "-U", "--other-user", "-R", "--chroot",
        ],
        "doas" => &["-u", "-C"],
        "nice" => &["-n", "--adjustment"],
        "time" => &["-f", "--format", "-o", "--output"],
        "timeout" => &["-s", "--signal", "-k", "--kill-after"],
        "stdbuf" => &["-i", "--input", "-o", "--output", "-e", "--error"],
        "caffeinate" => &["-t", "-w"],
        "exec" => &["-a"],
        _ => &[],
    };
    list.contains(&flag)
}

/// 合写的短选项（`sudo -iu root`）里哪些字母带参数；None 表示这个包装命令不认合写
fn short_takes_arg(wrapper: &str) -> Option<&'static [u8]> {
    Some(match wrapper {
        "sudo" => b"ugpCDhrtUR",
        "doas" => b"uC",
        "env" => b"uCS",
        "nice" => b"n",
        "timeout" => b"sk",
        "stdbuf" => b"ioe",
        "caffeinate" => b"tw",
        "exec" => b"a",
        _ => return None,
    })
}

const SH_KEYWORDS: &[&str] =
    &["[", "[[", "for", "if", "while", "until", "case", "do", "then", "else", "elif", "function", "select", "coproc", "done", "fi", "esac"];
const DECLARE: &[&str] = &["export", "local", "declare", "readonly", "typeset"];
const PS_KEYWORDS: &[&str] = &[
    "if", "elseif", "else", "for", "foreach", "while", "do", "switch", "function", "filter", "param", "begin", "process", "end",
    "try", "catch", "finally", "trap", "return", "throw", "break", "continue", "using", "class", "enum",
];

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&b| is_ws(b)).count();
    &s[n..]
}

fn is_blank(s: &[u8]) -> bool {
    s.iter().all(|&b| is_ws(b))
}

fn find(s: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if from > s.len() {
        return None;
    }
    s[from..].windows(pat.len()).position(|w| w == pat).map(|i| i + from)
}

fn find_byte(s: &[u8], from: usize, b: u8) -> Option<usize> {
    s.get(from..)?.iter().position(|&c| c == b).map(|i| i + from)
}

/// 前导的空格 / 制表符有多长
fn blanks(s: &[u8]) -> usize {
    s.iter().take_while(|&&b| b == b' ' || b == b'\t').count()
}

/// `^[ \t]*(?:&&|;|\r?\n)[ \t]*`（pwsh 的顺序不同，结果一样）：命中返回整段长度
fn chain_sep(s: &[u8]) -> Option<usize> {
    let mut i = blanks(s);
    let r = &s[i..];
    i += if r.starts_with(b"&&") || r.starts_with(b"\r\n") {
        2
    } else if r.starts_with(b";") || r.starts_with(b"\n") {
        1
    } else {
        return None;
    };
    Some(i + blanks(&s[i..]))
}

/// bash 双引号里 `\` 能转义的字符
fn dq_escapable(b: u8) -> bool {
    matches!(b, b'\\' | b'$' | b'`' | b'"' | b'\n')
}

/// 读一个 shell 词：去掉引号、处理转义，到空白或 `; & | ) }` 为止。返回 (词, 读到哪)
fn word(s: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut t = 0;
    let mut w = Vec::new();
    while t < s.len() {
        let r = s[t];
        if r == b'"' || r == b'\'' {
            t += 1;
            while t < s.len() && s[t] != r {
                if r == b'"' && s[t] == b'\\' && t + 1 < s.len() && dq_escapable(s[t + 1]) {
                    t += 1;
                }
                w.push(s[t]);
                t += 1;
            }
            if t < s.len() {
                t += 1;
            }
        } else if r == b'\\' && t + 1 < s.len() && is_ws(s[t + 1]) {
            w.push(s[t + 1]);
            t += 2;
        } else {
            if is_ws(r) || matches!(r, b';' | b'&' | b'|' | b')' | b'}') {
                break;
            }
            w.push(r);
            t += 1;
        }
    }
    if w.is_empty() { None } else { Some((w, t)) }
}

/// 从 start 起配对括号，返回配上的 `)` 之后的位置
fn paren_end(s: &[u8], start: usize) -> usize {
    let mut depth = 0;
    let mut r = start;
    while r < s.len() {
        let c = s[r];
        if c == b'"' || c == b'\'' {
            match find_byte(s, r + 1, c) {
                Some(i) => r = i,
                None => return s.len(),
            }
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return r + 1;
            }
        }
        r += 1;
    }
    s.len()
}

fn word_end(b: u8) -> bool {
    is_ws(b) || matches!(b, b';' | b'&' | b'|')
}

fn has_expansion(s: &[u8]) -> bool {
    s.iter().any(|&b| b == b'$' || b == b'`')
}

/// 双引号 / 单引号里的内容（bash 转义规则），返回 (内容, 闭引号之后的位置)
fn quoted(s: &[u8], at: usize, escape: u8) -> (Vec<u8>, usize) {
    let q = s[at];
    let mut t = at + 1;
    let mut n = Vec::new();
    while t < s.len() && s[t] != q {
        let esc = if escape == b'\\' { s[t] == b'\\' && t + 1 < s.len() && dq_escapable(s[t + 1]) } else { s[t] == escape && t + 1 < s.len() };
        if q == b'"' && esc {
            t += 1;
        }
        n.push(s[t]);
        t += 1;
    }
    (n, if t < s.len() { t + 1 } else { s.len() })
}

struct Assign {
    name: String,
    value: Option<String>,
    end: usize,
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// bash 赋值 `NAME=value` / `NAME+=value`
fn assignment(s: &[u8]) -> Option<Assign> {
    if !s.first().is_some_and(|&b| is_ident_start(b)) {
        return None;
    }
    let name_len = s.iter().take_while(|&&b| is_ident(b)).count();
    let mut r = name_len;
    let append = s.get(r) == Some(&b'+');
    if append {
        r += 1;
    }
    if s.get(r) != Some(&b'=') {
        return None;
    }
    r += 1;
    let mut value = None;
    match s.get(r) {
        Some(&q) if q == b'"' || q == b'\'' => {
            let (n, end) = quoted(s, r, b'\\');
            r = end;
            if q == b'\'' || !has_expansion(&n) {
                value = Some(lossy(&n));
            }
        }
        _ if s[r..].starts_with(b"$(") => r = paren_end(s, r + 1),
        Some(b'`') => r = find_byte(s, r + 1, b'`').map_or(s.len(), |i| i + 1),
        Some(b'(') => r = paren_end(s, r),
        _ => {
            let t = r;
            while r < s.len() && !word_end(s[r]) {
                r += 1;
            }
            let n = &s[t..r];
            if !has_expansion(n) {
                value = Some(lossy(n));
            }
        }
    }
    // 值后面还粘着别的（`A="x"y`）：值拿不准
    let mut o = r;
    while o < s.len() && !word_end(s[o]) {
        let c = s[o];
        if c == b'"' || c == b'\'' {
            match find_byte(s, o + 1, c) {
                Some(i) => o = i + 1,
                None => {
                    o = s.len();
                    break;
                }
            }
        } else {
            o += 1;
        }
    }
    if o > r {
        r = o;
        value = None;
    }
    Some(Assign { name: lossy(&s[..name_len]), value: if append { None } else { value }, end: r })
}

/// `export A=1 B=2 rest` → rest，顺手记下变量
fn strip_declare<'a>(s: &'a [u8], vars: &mut Vars) -> Option<&'a [u8]> {
    let (w, end) = word(s)?;
    if !DECLARE.iter().any(|d| d.as_bytes() == w.as_slice()) {
        return None;
    }
    let mut r = end;
    for _ in 0..16 {
        let b = blanks(&s[r..]);
        if b == 0 {
            break;
        }
        r += b;
        let rest = &s[r..];
        if rest.is_empty() || matches!(rest[0], b';' | b'|' | b'&' | b'#') {
            break;
        }
        if let Some(a) = assignment(rest) {
            r += a.end;
            vars.insert(a.name, a.value);
            continue;
        }
        let Some((_, e)) = word(rest) else { break };
        r += e;
    }
    Some(&s[r..])
}

/// 剥掉开头的重定向（`2>&1`、`> log`、`&> x`）
fn strip_redirs(mut s: &[u8]) -> &[u8] {
    for _ in 0..8 {
        let mut i = s.iter().take_while(|&&b| is_ws(b)).count();
        if s[i..].starts_with(b"&>") {
            i += 2;
        } else {
            i += s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
            let arrows = s[i..].iter().take_while(|&&b| b == b'<' || b == b'>').count();
            if arrows == 0 {
                break;
            }
            i += arrows;
            if s.get(i) == Some(&b'&') {
                i += 1;
            }
        }
        i += s[i..].iter().take_while(|&&b| is_ws(b)).count();
        s = &s[i..];
        let Some((_, end)) = word(s) else { break };
        s = &s[end..];
    }
    s
}

/// `cd X && rest` → rest（bash）
fn strip_cd(s: &[u8]) -> Option<&[u8]> {
    if !s.starts_with(b"cd") {
        return None;
    }
    let b = blanks(&s[2..]);
    if b == 0 {
        return None;
    }
    let n = &s[2 + b..];
    let mut r = 0;
    for _ in 0..4 {
        let rest = &n[r..];
        let flag = if rest.starts_with(b"--") {
            2
        } else if rest.first() == Some(&b'-') {
            let k = rest[1..].iter().take_while(|&&c| matches!(c, b'P' | b'L' | b'@' | b'e')).count();
            if k == 0 { break } else { k + 1 }
        } else {
            break;
        };
        let b = blanks(&rest[flag..]);
        if b == 0 {
            break;
        }
        r += flag + b;
    }
    let (_, end) = word(&n[r..])?;
    let a = strip_redirs(&n[r + end..]);
    let rest = &a[chain_sep(a)?..];
    if is_blank(rest) { None } else { Some(rest) }
}

fn starts_with_ci(s: &[u8], p: &str) -> bool {
    s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p.as_bytes())
}

/// `cd X; rest` / `Set-Location -Path X; rest` → rest（PowerShell）
fn strip_cd_pwsh(s: &[u8]) -> Option<&[u8]> {
    let kw = ["Set-Location", "cd", "sl"].into_iter().find(|k| starts_with_ci(s, k) && !s.get(k.len()).is_some_and(|&b| is_ident(b)))?;
    let n = &s[kw.len() + blanks(&s[kw.len()..])..];
    let mut r = 0;
    for _ in 0..8 {
        let rest = &n[r..];
        if rest.first() != Some(&b'-') {
            break;
        }
        let named = ["-Path", "-LiteralPath"].into_iter().find(|p| starts_with_ci(rest, p) && blanks(&rest[p.len()..]) > 0);
        if let Some(p) = named {
            r += p.len() + blanks(&rest[p.len()..]);
            continue;
        }
        let k = rest[1..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
        if k == 0 {
            break;
        }
        r += 1 + k + blanks(&rest[1 + k..]);
    }
    let (_, end) = word(&n[r..])?;
    let a = strip_redirs(&n[r + end..]);
    let rest = &a[chain_sep(a)?..];
    if is_blank(rest) { None } else { Some(rest) }
}

/// 第一个词去掉路径和 .exe / .cmd / .bat
fn basename(w: &str) -> String {
    let t = w.trim();
    let t = t.rfind(['/', '\\']).map_or(t, |i| &t[i + 1..]);
    let lower = t.to_ascii_lowercase();
    let t = match t.rfind('.') {
        Some(i) if i > 0 && [".exe", ".cmd", ".bat"].iter().any(|e| lower.ends_with(e)) => &t[..i],
        _ => t,
    };
    t.trim().to_string()
}

/// `sudo -u root env A=1 cmd` → `env A=1 cmd`：剥一层包装命令
fn strip_wrapper(s: &[u8]) -> Option<&[u8]> {
    let (w, end) = word(s)?;
    let name = basename(&lossy(&w));
    if !WRAPPERS.contains(&name.as_str()) {
        return None;
    }
    // `command -v x` 是查询，不是执行
    if name == "command" {
        let rest = &s[end..];
        let ws = rest.iter().take_while(|&&b| is_ws(b)).count();
        if ws > 0 && rest.get(ws) == Some(&b'-') {
            let flag: Vec<u8> = rest[ws + 1..].iter().copied().take_while(u8::is_ascii_alphabetic).collect();
            if flag.iter().any(|&b| b == b'v' || b == b'V') {
                return None;
            }
        }
    }
    let short = short_takes_arg(&name);
    let mut i = end;
    let mut duration = false;
    for _ in 0..24 {
        let ws = s[i..].iter().take_while(|&&b| is_ws(b)).count();
        if ws == 0 {
            break;
        }
        i += ws;
        let o = &s[i..];
        if o.is_empty() || matches!(o[0], b';' | b'|' | b'&' | b'#') {
            return None;
        }
        if o.starts_with(b"--") && o.get(2).is_none_or(|&b| is_ws(b)) {
            i += 2;
            continue;
        }
        if matches!(name.as_str(), "env" | "sudo" | "doas")
            && let Some(a) = assignment(o)
        {
            i += a.end;
            continue;
        }
        // -{1,2}[A-Za-z0-9][A-Za-z0-9_-]*(=\S+)?
        let dashes = o.iter().take(2).take_while(|&&b| b == b'-').count();
        if dashes > 0 && o.get(dashes).is_some_and(u8::is_ascii_alphanumeric) {
            let flag_len = dashes + 1 + o[dashes + 1..].iter().take_while(|&&b| is_ident(b) || b == b'-').count();
            let mut full = flag_len;
            let has_eq = o.get(flag_len) == Some(&b'=') && o.get(flag_len + 1).is_some_and(|&b| !is_ws(b));
            if has_eq {
                full += 1 + o[flag_len + 1..].iter().take_while(|&&b| !is_ws(b)).count();
            }
            i += full;
            let flag = lossy(&o[..flag_len]);
            let long = dashes == 2;
            let mut takes = !has_eq && takes_arg(&name, &flag);
            if !long && flag_len != 2 && !has_eq
                && let Some(letters) = short
            {
                let chars = &flag.as_bytes()[1..];
                if let Some(at) = chars.iter().position(|c| letters.contains(c)) {
                    takes = at == chars.len() - 1;
                }
            }
            if takes {
                let b = blanks(&s[i..]);
                if let Some((_, e)) = word(&s[i + b..]) {
                    i += b + e;
                }
            }
        } else {
            // timeout 的第一个位置参数是时长
            if name == "timeout" && !duration
                && let Some((_, e)) = word(o)
            {
                i += e;
                duration = true;
                continue;
            }
            break;
        }
    }
    let o = trim_start(strip_redirs(&s[i..]));
    if o.is_empty() || matches!(o[0], b';' | b'|' | b'&' | b'#') { None } else { Some(o) }
}

/// `$NAME` / `${NAME}`：返回 (名字, 引用的长度)；pwsh 允许 `$env:PATH` 这样的作用域
fn var_ref(w: &[u8], pwsh: bool, scopes: usize) -> Option<(String, usize)> {
    if w.first() != Some(&b'$') {
        return None;
    }
    if w.get(1) == Some(&b'{') {
        let close = find_byte(w, 2, b'}')?;
        let inner = &w[2..close];
        let ok = if pwsh { !inner.is_empty() } else { !inner.is_empty() && is_ident_start(inner[0]) && inner.iter().all(|&b| is_ident(b)) };
        return ok.then(|| (lossy(inner).trim_start_matches('$').to_string(), close + 1));
    }
    if !w.get(1).is_some_and(|&b| is_ident_start(b)) {
        return None;
    }
    let mut r = 1 + w[1..].iter().take_while(|&&b| is_ident(b)).count();
    if pwsh {
        for _ in 0..scopes {
            if w.get(r) == Some(&b':') && w.get(r + 1).is_some_and(|&b| is_ident_start(b)) {
                r += 1 + w[r + 1..].iter().take_while(|&&b| is_ident(b) || b == b'-').count();
            } else {
                break;
            }
        }
    }
    Some((lossy(&w[1..r]), r))
}

/// 命令词里的变量换成赋值记下的值；拿不准就 None（记作 shell）
fn resolve(w: &[u8], vars: &Vars, pwsh: bool) -> Option<String> {
    if w.is_empty() || w.starts_with(b"$(") {
        return None;
    }
    if !pwsh && (w[0] == b'`' || w.contains(&b'`')) {
        return None;
    }
    if let Some((name, len)) = var_ref(w, pwsh, usize::MAX)
        && len == w.len()
    {
        return vars.get(&name).cloned().flatten();
    }
    if let Some((name, len)) = var_ref(w, pwsh, 1) {
        let rest = &w[len..];
        if has_expansion(rest) {
            return None;
        }
        if let Some(Some(v)) = vars.get(&name) {
            return Some(v.clone() + &lossy(rest));
        }
        // 没记下值：`$HOME/bin/tool` 取最后一段
        let tail = rest.iter().rposition(|&b| b == b'/' || b == b'\\').map(|i| &rest[i + 1..]).unwrap_or(&[]);
        return if tail.is_empty() { None } else { Some(lossy(tail)) };
    }
    if w.contains(&b'$') || (pwsh && w.contains(&b'`')) { None } else { Some(lossy(w)) }
}

/// PowerShell 语句的结尾：深度为 0 的 `;` 或换行
fn stmt_end(s: &[u8], from: usize) -> usize {
    let mut depth = 0;
    let mut r = from;
    while r < s.len() {
        let c = s[r];
        if c == b'"' || c == b'\'' {
            match find_byte(s, r + 1, c) {
                Some(i) => r = i,
                None => return s.len(),
            }
        } else if c == b'#' {
            match find_byte(s, r, b'\n') {
                Some(i) => r = i - 1,
                None => return s.len(),
            }
        } else if matches!(c, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(c, b')' | b']' | b'}') {
            if depth > 0 {
                depth -= 1;
            }
        } else if depth == 0 && (c == b';' || c == b'\n') {
            return r;
        }
        r += 1;
    }
    s.len()
}

/// 复合赋值的运算符 `+ - * / % ??`，后面紧跟 `=`；返回 (是不是复合, 到 `=` 之后的位置)
fn assign_op(s: &[u8], at: usize) -> Option<(bool, usize)> {
    let op = if s[at..].starts_with(b"??") {
        2
    } else if s.get(at).is_some_and(|b| b"+-*/%".contains(b)) {
        1
    } else {
        0
    };
    if s.get(at + op) == Some(&b'=') {
        return Some((op > 0, at + op + 1));
    }
    if op > 0 && s.get(at) == Some(&b'=') {
        return Some((false, at + 1));
    }
    None
}

/// `[Type]::Prop = …`（静态属性赋值）
fn static_assign(s: &[u8]) -> bool {
    let mut i = s.iter().take_while(|&&b| is_ws(b)).count();
    if s.get(i) != Some(&b'[') {
        return false;
    }
    let Some(close) = find_byte(s, i + 1, b']') else { return false };
    if close == i + 1 || !s[close + 1..].starts_with(b"::") || !s.get(close + 3).is_some_and(|&b| is_ident_start(b)) {
        return false;
    }
    i = close + 3;
    i += s[i..].iter().take_while(|&&b| is_ident(b) || b == b'.').count();
    i += s[i..].iter().take_while(|&&b| is_ws(b)).count();
    assign_op(s, i).is_some()
}

/// PowerShell 赋值 `$x = …`、`[int]$x += 1`、`$env:PATH = …`
fn assignment_pwsh(s: &[u8]) -> Option<Assign> {
    let mut p = s.iter().take_while(|&&b| is_ws(b)).count();
    if s.get(p) == Some(&b'[')
        && let Some(close) = find_byte(s, p + 1, b']')
        && close > p + 1
    {
        p = close + 1;
        p += s[p..].iter().take_while(|&&b| is_ws(b)).count();
    }
    if s.get(p) != Some(&b'$') || !s.get(p + 1).is_some_and(|&b| is_ident_start(b)) {
        return None;
    }
    let start = p + 1;
    p = start + s[start..].iter().take_while(|&&b| is_ident(b)).count();
    loop {
        match s.get(p) {
            Some(b':') if s.get(p + 1).is_some_and(|&b| is_ident_start(b)) => {
                p += 1 + s[p + 1..].iter().take_while(|&&b| is_ident(b) || b == b'-').count();
            }
            Some(b'[') => match find_byte(s, p + 1, b']') {
                Some(c) => p = c + 1,
                None => break,
            },
            Some(b'.') if s.get(p + 1).is_some_and(|&b| is_ident_start(b)) => {
                p += 1 + s[p + 1..].iter().take_while(|&&b| is_ident(b)).count();
            }
            _ => break,
        }
    }
    let name = lossy(&s[start..p]);
    p += s[p..].iter().take_while(|&&b| is_ws(b)).count();
    let (compound, after) = assign_op(s, p)?;
    let r = after + s[after..].iter().take_while(|&&b| is_ws(b)).count();
    let mut value = None;
    let o = match s.get(r) {
        Some(&q) if q == b'"' || q == b'\'' => {
            let (n, mut o) = quoted(s, r, b'`');
            if q == b'\'' || !has_expansion(&n) {
                value = Some(lossy(&n));
            }
            let rest = &s[o..];
            let b = blanks(rest);
            let ends = rest.len() == b || matches!(rest[b], b';' | b'#' | b'\n') || rest[b..].starts_with(b"\r\n");
            if !ends {
                o = stmt_end(s, o);
                value = None;
            }
            o
        }
        _ => stmt_end(s, r),
    };
    Some(Assign { name, value: if compound { None } else { value }, end: o })
}

/// 一段 bash 命令的主程序
fn program_bash(cmd: &[u8], vars: &mut Vars) -> String {
    if is_blank(cmd) {
        return String::new();
    }
    let mut n = trim_start(cmd);
    for _ in 0..32 {
        let k = n.iter().take_while(|&&b| is_ws(b) || matches!(b, b';' | b'&' | b'|')).count();
        n = &n[k..];
        if n.is_empty() {
            break;
        }
        if n[0] == b'#' {
            let Some(i) = find_byte(n, 0, b'\n') else { break };
            n = &n[i + 1..];
            continue;
        }
        if n.starts_with(b"\\\n") || n.starts_with(b"\\\r\n") {
            let k = if n[1] == b'\r' { 3 } else { 2 };
            n = &n[k + blanks(&n[k..])..];
            continue;
        }
        if n[0] == b'!' && n.get(1).is_some_and(|&b| is_ws(b)) {
            n = trim_start(&n[1..]);
            continue;
        }
        let e = strip_redirs(n);
        if e.len() != n.len() {
            n = trim_start(e);
            if n.is_empty() {
                break;
            }
            continue;
        }
        if let Some(r) = strip_cd(n) {
            n = r;
            continue;
        }
        if let Some(r) = strip_declare(n, vars) {
            n = r;
            continue;
        }
        if let Some(a) = assignment(n) {
            n = trim_start(&n[a.end..]);
            vars.insert(a.name, a.value);
            continue;
        }
        match strip_wrapper(n) {
            Some(r) => n = r,
            None => break,
        }
    }
    const SHELL: &str = "shell";
    if n.is_empty() || matches!(n[0], b'#' | b'(' | b'{') {
        return SHELL.into();
    }
    let Some((w, end)) = word(n) else { return SHELL.into() };
    let rest = &n[end..];
    // 函数定义 `f() {` / `f ()`
    let fn_word = w.last() == Some(&b'(') && is_ident_start(w[0]) && w[..w.len() - 1].iter().all(|&b| is_ident(b) || b == b'-');
    let rest_t = trim_start(rest);
    if (fn_word && rest.first() == Some(&b')')) || (rest_t.first() == Some(&b'(') && trim_start(&rest_t[1..]).first() == Some(&b')')) {
        return SHELL.into();
    }
    let Some(i) = resolve(&w, vars, false).filter(|s| !s.is_empty()) else { return SHELL.into() };
    let head = i.split('(').next().unwrap_or("");
    if SH_KEYWORDS.contains(&i.as_str()) || SH_KEYWORDS.contains(&head) {
        return SHELL.into();
    }
    basename(&i)
}

fn is_number_word(w: &[u8]) -> bool {
    // \d+(\.\d+)?(\.\.\d+(\.\d+)?)?
    fn num(w: &[u8]) -> bool {
        let mut parts = w.splitn(2, |&b| b == b'.');
        let a = parts.next().unwrap_or(&[]);
        let ok = |p: &[u8]| !p.is_empty() && p.iter().all(u8::is_ascii_digit);
        ok(a) && parts.next().is_none_or(ok)
    }
    match find(w, 0, b"..") {
        Some(i) => num(&w[..i]) && num(&w[i + 2..]),
        None => num(w),
    }
}

/// 一段 PowerShell 命令的主程序
fn program_pwsh(cmd: &[u8], vars: &mut Vars) -> String {
    if is_blank(cmd) {
        return String::new();
    }
    let mut n = trim_start(cmd);
    for _ in 0..32 {
        let k = n.iter().take_while(|&&b| is_ws(b) || b == b';').count();
        n = &n[k..];
        if n.is_empty() {
            break;
        }
        if n.starts_with(b"<#") {
            n = find(n, 2, b"#>").map_or(&[][..], |i| trim_start(&n[i + 2..]));
            continue;
        }
        if n[0] == b'#' {
            let Some(i) = find_byte(n, 0, b'\n') else { break };
            n = &n[i + 1..];
            continue;
        }
        let e = strip_redirs(n);
        if e.len() != n.len() {
            n = trim_start(e);
            if n.is_empty() {
                break;
            }
            continue;
        }
        if let Some(r) = strip_cd_pwsh(n) {
            n = r;
            continue;
        }
        if static_assign(n) {
            n = trim_start(&n[stmt_end(n, 0)..]);
            continue;
        }
        match assignment_pwsh(n) {
            Some(a) => {
                n = trim_start(&n[a.end.min(n.len())..]);
                vars.insert(a.name, a.value);
            }
            None => break,
        }
    }
    const PWSH: &str = "pwsh";
    if n.is_empty() || n[0] == b'#' {
        return PWSH.into();
    }
    // `& "path\tool.exe"`、`. script.ps1`：调用运算符后面是命令
    let mut call = false;
    if n[0] == b'&' {
        call = true;
        n = trim_start(&n[1..]);
    } else if n[0] == b'.' && n.get(1).is_some_and(|&b| is_ws(b)) {
        call = true;
        n = trim_start(&n[1..]);
    }
    if n.is_empty() {
        return PWSH.into();
    }
    let quoted = n[0] == b'"' || n[0] == b'\'';
    let Some((w, _)) = word(n) else { return PWSH.into() };
    if matches!(w[0], b'(' | b'[' | b'{' | b'@') {
        return PWSH.into();
    }
    let operator = ["-not", "-join", "-split"].iter().any(|op| starts_with_ci(&w, op) && !w.get(op.len()).is_some_and(|&b| is_ident(b)));
    if !call && (quoted || w[0] == b'$' || w[0] == b'!' || is_number_word(&w) || operator) {
        return PWSH.into();
    }
    let Some(c) = resolve(&w, vars, true).filter(|s| !s.is_empty()) else { return PWSH.into() };
    let lower = c.to_ascii_lowercase();
    let head = lower.split('(').next().unwrap_or("");
    if PS_KEYWORDS.contains(&lower.as_str()) || PS_KEYWORDS.contains(&head) {
        return PWSH.into();
    }
    basename(&c)
}

fn program(cmd: &[u8], d: Dialect, vars: &mut Vars) -> String {
    match d {
        Dialect::Bash => program_bash(cmd, vars),
        Dialect::Pwsh => program_pwsh(cmd, vars),
    }
}

/// heredoc 的正文：从 at 起跳过每个 heredoc 直到它的结束行，返回之后的位置
fn skip_heredocs(s: &[u8], at: usize, docs: &[(Vec<u8>, bool)]) -> usize {
    let mut r = at;
    for (delim, strip_tabs) in docs {
        let mut found = false;
        while r < s.len() {
            let nl = find_byte(s, r, b'\n');
            let mut line = &s[r..nl.unwrap_or(s.len())];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if *strip_tabs {
                line = &line[line.iter().take_while(|&&b| b == b'\t').count()..];
            }
            r = nl.map_or(s.len(), |i| i + 1);
            if line == delim.as_slice() {
                found = true;
                break;
            }
        }
        if !found {
            return s.len();
        }
    }
    r
}

/// 收一段：[start, end) 不空就记下，下一段从 next 开始
fn cut<'a>(s: &'a [u8], out: &mut Vec<(&'a [u8], &'static str)>, start: &mut usize, end: usize, sep: &'static str, next: usize) {
    let seg = &s[*start..end.min(s.len())];
    if !is_blank(seg) {
        out.push((seg, sep));
    }
    *start = next;
}

/// 按分隔符切段：(段文本, 段后面的分隔符)
fn segments(s: &[u8], d: Dialect) -> Vec<(&[u8], &'static str)> {
    let bash = d == Dialect::Bash;
    let mut out = Vec::new();
    let mut docs: Vec<(Vec<u8>, bool)> = Vec::new();
    let (mut start, mut a, mut parens, mut braces) = (0usize, 0usize, 0u32, 0u32);
    while a < s.len() {
        let c = s[a];
        let next = s.get(a + 1).copied();
        if (bash && c == b'\\') || (!bash && c == b'`') {
            a += 2;
            continue;
        }
        if !bash && c == b'<' && next == Some(b'#') {
            a = find(s, a + 2, b"#>").map_or(s.len(), |i| i + 2);
            continue;
        }
        if !bash && c == b'@' && matches!(next, Some(b'"' | b'\'')) {
            let after = &s[a + 2..];
            if after.starts_with(b"\n") || after.starts_with(b"\r\n") {
                let term = [b'\n', next.unwrap(), b'@'];
                a = find(s, a + 2, &term).map_or(s.len(), |i| i + term.len());
                continue;
            }
        }
        if c == b'"' || c == b'\'' {
            let mut i = a + 1;
            while i < s.len() {
                let n = s[i];
                let esc = c == b'"'
                    && i + 1 < s.len()
                    && ((bash && n == b'\\' && dq_escapable(s[i + 1])) || (!bash && n == b'`'));
                if esc {
                    i += 2;
                    continue;
                }
                if n == c {
                    // PowerShell 的 '' / "" 是转义的引号
                    if !bash && s.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            a = i + 1;
            continue;
        }
        if c == b'#' && (a == 0 || is_ws(s[a - 1]) || matches!(s[a - 1], b';' | b'&' | b'|')) {
            a = find_byte(s, a, b'\n').unwrap_or(s.len());
            continue;
        }
        if bash && c == b'<' && next == Some(b'<') && s.get(a + 2) != Some(&b'<') {
            let mut t = a + 2;
            let strip = s.get(t) == Some(&b'-');
            if strip {
                t += 1;
            }
            t += blanks(&s[t.min(s.len())..]);
            if let Some((w, end)) = word(&s[t.min(s.len())..]) {
                docs.push((w, strip));
                a = t + end;
                continue;
            }
        }
        match c {
            b'(' => parens += 1,
            b')' if parens > 0 => parens -= 1,
            b'{' => braces += 1,
            b'}' if braces > 0 => braces -= 1,
            _ => {}
        }
        let top = parens == 0 && braces == 0;
        if c == b'\n' {
            if !docs.is_empty() {
                let t = skip_heredocs(s, a + 1, &docs);
                if top {
                    cut(s, &mut out, &mut start, a, "\n", t);
                }
                docs.clear();
                a = t;
                continue;
            }
            if top {
                cut(s, &mut out, &mut start, a, "\n", a + 1);
                a += 1;
                continue;
            }
        } else if top {
            let (sep, w): (&'static str, usize) = match c {
                b';' => (";", 1),
                b'|' if next == Some(b'|') => ("||", 2),
                b'|' => ("|", if next == Some(b'&') { 2 } else { 1 }),
                b'&' if next == Some(b'&') => ("&&", 2),
                b'&' if bash && (a == 0 || s[a - 1] != b'>') && next != Some(b'>') => ("&", 1),
                _ => ("", 0),
            };
            if w > 0 {
                cut(s, &mut out, &mut start, a, sep, a + w);
                a += w;
                continue;
            }
        }
        a += 1;
    }
    cut(s, &mut out, &mut start, s.len(), "", s.len());
    out
}

/// 一条命令用到的程序：整条命令的主程序在前，其余各段的程序跟在后面（按出现顺序，可能重复）
pub fn programs(cmd: &str, d: Dialect) -> Vec<String> {
    let cmd = cmd.as_bytes();
    let cmd = &cmd[..cmd.len().min(MAX_CMD)];
    if is_blank(cmd) {
        return Vec::new();
    }
    let fallback = match d {
        Dialect::Bash => "shell",
        Dialect::Pwsh => "pwsh",
    };
    let main = program(cmd, d, &mut Vars::new());
    let mut vars = Vars::new();
    let mut others = Vec::new();
    for (seg, sep) in segments(cmd, d) {
        let p = program(seg, d, &mut vars);
        let is_cd = match d {
            Dialect::Bash => p == "cd",
            Dialect::Pwsh => ["cd", "set-location", "sl"].contains(&p.to_ascii_lowercase().as_str()),
        };
        if is_cd && matches!(sep, "&&" | ";" | "\n") {
            continue;
        }
        if p != fallback {
            others.push(p);
        }
    }
    if main != fallback
        && let Some(i) = others.iter().position(|p| *p == main)
    {
        others.remove(i);
    }
    let main = if main.is_empty() { fallback.to_string() } else { main };
    std::iter::once(main).chain(others).collect()
}

fn str_of<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

/// Codex 在 Windows 上用 PowerShell 跑命令；看工作目录是不是盘符路径来猜
fn windows_workdir(input: &Value) -> bool {
    str_of(input, "workdir").is_some_and(|w| {
        let b = w.as_bytes();
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    })
}

/// 一次 shell 工具调用用到的程序（去重，主程序在前）；不是 shell 调用、拿不到命令就空
///
/// 认的形状：Claude Code / pfh / dsh 的 `bash`（`command`）、`PowerShell` / `pwsh`（`command`）、
/// Codex 的 `shell`（`command` 是 argv）、`shell_command`（`command`）、`exec_command`（`cmd`），dsh 的 `run_code`
pub fn programs_of_call(tool: &str, input: Option<&Value>) -> Vec<String> {
    let name = tool.to_ascii_lowercase();
    if name == "run_code" {
        return vec!["run_code".into()];
    }
    let Some(input) = input else { return Vec::new() };
    // dsh 解析不了的参数原样是字符串
    let parsed;
    let input = match input {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(v @ Value::Object(_)) => {
                parsed = v;
                &parsed
            }
            _ => return Vec::new(),
        },
        v => v,
    };
    let list = match name.as_str() {
        "bash" | "shell" | "exec" if input.get("command").is_some_and(Value::is_array) => argv_programs(input),
        "bash" | "exec" => str_of(input, "command").map(|c| programs(c, Dialect::Bash)).unwrap_or_default(),
        "powershell" | "pwsh" => str_of(input, "command").map(|c| programs(c, Dialect::Pwsh)).unwrap_or_default(),
        "shell" | "shell_command" | "exec_command" => {
            let d = if windows_workdir(input) { Dialect::Pwsh } else { Dialect::Bash };
            str_of(input, "command").or_else(|| str_of(input, "cmd")).map(|c| programs(c, d)).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for p in list {
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// argv 形式（Codex 老版 `shell`）：`bash -lc "<script>"`、`powershell -Command "<script>"` 解析脚本，否则 argv[0] 就是程序
fn argv_programs(input: &Value) -> Vec<String> {
    let argv: Vec<&str> = input.get("command").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let Some(first) = argv.first() else { return Vec::new() };
    let exe = basename(first).to_ascii_lowercase();
    let script_after = |flags: &[&str]| {
        argv.iter().position(|a| flags.iter().any(|f| a.eq_ignore_ascii_case(f))).and_then(|i| argv.get(i + 1)).copied()
    };
    match exe.as_str() {
        "bash" | "sh" | "zsh" | "dash" => match script_after(&["-c", "-lc", "-ic", "-lic"]) {
            Some(s) => programs(s, Dialect::Bash),
            None => vec![exe],
        },
        "powershell" | "pwsh" => match script_after(&["-Command", "-c"]) {
            Some(s) => programs(s, Dialect::Pwsh),
            None => vec![exe],
        },
        _ => vec![basename(first)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash(c: &str) -> Vec<String> {
        programs(c, Dialect::Bash)
    }

    fn pwsh(c: &str) -> Vec<String> {
        programs(c, Dialect::Pwsh)
    }

    #[test]
    fn wrappers_cd_and_pipes() {
        assert_eq!(bash("cd x && sudo env A=1 npm test | tee log"), ["npm", "tee"]);
        assert_eq!(bash("FOO=bar python -m x; ls -la"), ["python", "ls"]);
        assert_eq!(bash("sudo -u root -E nice -n 5 timeout 30 cargo build"), ["cargo"]);
        assert_eq!(bash("timeout -s KILL 10s ./run.sh --flag"), ["run.sh"]);
        assert_eq!(bash("env -u HOME PATH=/x /usr/bin/git status 2>&1 | head -5"), ["git", "head"]);
        assert_eq!(bash("command -v git"), ["command"]);
        assert_eq!(bash("cd /e/code/x && git status && cd sub; ls"), ["git", "ls"]);
    }

    #[test]
    fn quotes_comments_and_heredoc() {
        // 引号里的分隔符不切
        assert_eq!(bash(r#"echo "a; b | c" && grep -n 'x && y' f"#), ["echo", "grep"]);
        assert_eq!(bash("git log # | not a pipe\nwc -l"), ["git", "wc"]);
        // heredoc 正文里的东西不当命令
        assert_eq!(bash("cat <<'EOF' > out.txt\nrm -rf /\nfoo | bar\nEOF\nls"), ["cat", "ls"]);
        assert_eq!(bash("python - <<-EOF\n\timport os; os.system('x')\n\tEOF"), ["python"]);
        // $() 和子 shell 里的不切
        assert_eq!(bash("echo $(date; whoami) && (cd a; make)"), ["echo"]);
    }

    #[test]
    fn variables_keywords_and_paths() {
        assert_eq!(bash("PY=/usr/bin/python3; $PY -V"), ["python3"]);
        assert_eq!(bash("export CC=clang && $CC main.c"), ["clang"]);
        assert_eq!(bash("$HOME/bin/tool run"), ["tool"]);
        // 循环体前面的 do 也是关键字：和 dsh-context 一样整段记作 shell
        assert_eq!(bash("for f in *.rs; do rustfmt $f; done"), ["shell"]);
        assert_eq!(bash("C:/Tools/node.exe index.js"), ["node"]);
        assert_eq!(bash("f() { echo hi; }; f"), ["shell", "f"]);
        assert_eq!(bash("> out.log 2>&1 make all & wait"), ["make", "wait"]);
        assert!(bash("   ").is_empty());
    }

    #[test]
    fn powershell_pipeline() {
        assert_eq!(pwsh("Get-ChildItem -Recurse | Where-Object { $_.Length -gt 1kb } | Select-Object -First 5"), [
            "Get-ChildItem",
            "Where-Object",
            "Select-Object"
        ]);
        assert_eq!(pwsh("Set-Location -Path E:\\code; cargo test --workspace"), ["cargo"]);
        assert_eq!(pwsh("$exe = 'C:\\bin\\rg.exe'; & $exe -n foo"), ["rg"]);
        assert_eq!(pwsh("& \"C:\\Program Files\\Git\\bin\\git.exe\" status"), ["git"]);
        assert_eq!(pwsh("$env:RUST_LOG = \"debug\"; cargo run"), ["cargo"]);
        assert_eq!(pwsh("<# note; x | y #> npm ci"), ["npm"]);
        assert_eq!(pwsh("$s = @\"\nfoo | bar\n\"@\nWrite-Output $s"), ["Write-Output"]);
        assert_eq!(pwsh("foreach ($f in $files) { Remove-Item $f }"), ["pwsh"]);
        assert_eq!(pwsh("cd E:\\x; git status; if ($?) { pnpm build }"), ["git"]);
    }

    #[test]
    fn tool_shapes() {
        assert_eq!(programs_of_call("Bash", Some(&json!({"command": "ls | grep a | grep b"}))), ["ls", "grep"]);
        assert_eq!(programs_of_call("PowerShell", Some(&json!({"command": "git status"}))), ["git"]);
        assert_eq!(programs_of_call("shell", Some(&json!({"command": ["bash", "-lc", "ls -la && rg -n x || true"]}))), [
            "ls", "rg", "true"
        ]);
        assert_eq!(programs_of_call("shell", Some(&json!({"command": ["/usr/bin/git", "status"]}))), ["git"]);
        assert_eq!(programs_of_call("exec_command", Some(&json!({"cmd": "uv add x", "workdir": "E:\\code\\p"}))), ["uv"]);
        assert_eq!(programs_of_call("bash", Some(&json!(r#"{"command": "make"}"#))), ["make"]);
        assert_eq!(programs_of_call("run_code", None), ["run_code"]);
        assert!(programs_of_call("Read", Some(&json!({"file_path": "x"}))).is_empty());
        assert!(programs_of_call("write_stdin", Some(&json!({"chars": "y"}))).is_empty());
    }
}
