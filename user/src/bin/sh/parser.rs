//! Parser: tokens to an abstract syntax tree.
//!
//! ```text
//! list      := and_or ((';' | '&' | NL) and_or)*
//! and_or    := pipeline (('&&' | '||') NL* pipeline)*
//! pipeline  := '!'? command ('|' NL* command)*
//! command   := simple | compound redirect* | name '(' ')' compound
//! compound  := if | while | until | for | '{' list '}' | '(' list ')'
//! ```

use crate::lexer::{Part, Token, Word};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub enum RedirKind {
    In,
    Out,
    Append,
    Err,
    ErrAppend,
    ErrToOut,
    OutToErr,
}

#[derive(Clone, Debug)]
pub struct Redir {
    pub kind: RedirKind,
    pub target: Word,
}

#[derive(Clone, Debug)]
pub enum Command {
    Simple { assigns: Vec<(String, Word)>, words: Vec<Word> },
    If { branches: Vec<(List, List)>, otherwise: Option<List> },
    While { cond: List, body: List, until: bool },
    For { var: String, items: Option<Vec<Word>>, body: List },
    Group(List),
    Subshell(List),
    Function { name: String, body: Box<Node> },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub cmd: Command,
    pub redirs: Vec<Redir>,
}

#[derive(Clone, Debug)]
pub struct Pipeline {
    pub nodes: Vec<Node>,
    pub negate: bool,
}

#[derive(Clone, Debug)]
pub struct AndOr {
    pub first: Pipeline,
    /// (true = `&&`, false = `||`, pipeline)
    pub rest: Vec<(bool, Pipeline)>,
}

#[derive(Clone, Debug, Default)]
pub struct List {
    /// (command, run in background)
    pub items: Vec<(AndOr, bool)>,
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Incomplete,
    Syntax(String),
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

const RESERVED: &[&str] = &["then", "elif", "else", "fi", "do", "done", "}", "in"];

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_word(&self) -> Option<&str> {
        self.peek().and_then(Token::plain)
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Token::Op(o)) if *o == op)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Token::Newline)) {
            self.pos += 1;
        }
    }

    fn expect_word(&mut self, w: &str) -> Result<(), ParseError> {
        self.skip_newlines();
        match self.peek_word() {
            Some(x) if x == w => {
                self.pos += 1;
                Ok(())
            }
            Some(x) => Err(ParseError::Syntax(alloc::format!("expected '{}' but found '{}'", w, x))),
            None if self.peek().is_none() => Err(ParseError::Incomplete),
            None => Err(ParseError::Syntax(alloc::format!("expected '{}'", w))),
        }
    }

    fn expect_op(&mut self, op: &str) -> Result<(), ParseError> {
        self.skip_newlines();
        if self.is_op(op) {
            self.pos += 1;
            Ok(())
        } else if self.peek().is_none() {
            Err(ParseError::Incomplete)
        } else {
            Err(ParseError::Syntax(alloc::format!("expected '{}'", op)))
        }
    }

    /// A list that ends at one of the `terminators` (not consumed).
    fn list(&mut self, terminators: &[&str]) -> Result<List, ParseError> {
        let mut list = List::default();
        loop {
            self.skip_newlines();
            match self.peek() {
                None => {
                    if terminators.is_empty() {
                        return Ok(list);
                    }
                    return Err(ParseError::Incomplete);
                }
                Some(Token::Op(")")) if terminators.contains(&")") => return Ok(list),
                _ => {}
            }
            if let Some(w) = self.peek_word() {
                if terminators.contains(&w) {
                    return Ok(list);
                }
            }
            let and_or = self.and_or()?;
            let background = if self.is_op("&") {
                self.pos += 1;
                true
            } else {
                if self.is_op(";") {
                    self.pos += 1;
                }
                false
            };
            list.items.push((and_or, background));
            match self.peek() {
                None | Some(Token::Newline) | Some(Token::Op(")")) => {}
                Some(Token::Op(op)) if *op == ";" || *op == "&" => {}
                Some(t) if t.plain().is_some_and(|w| terminators.contains(&w)) => {}
                _ => {
                    if !matches!(self.tokens.get(self.pos - 1), Some(Token::Op(";" | "&"))) {
                        return Err(ParseError::Syntax("unexpected token".into()));
                    }
                }
            }
        }
    }

    fn and_or(&mut self) -> Result<AndOr, ParseError> {
        let first = self.pipeline()?;
        let mut rest = Vec::new();
        loop {
            let and = if self.is_op("&&") {
                true
            } else if self.is_op("||") {
                false
            } else {
                break;
            };
            self.pos += 1;
            self.skip_newlines();
            if self.peek().is_none() {
                return Err(ParseError::Incomplete);
            }
            rest.push((and, self.pipeline()?));
        }
        Ok(AndOr { first, rest })
    }

    fn pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let negate = if self.is_op("!") {
            self.pos += 1;
            true
        } else {
            false
        };
        let mut nodes = alloc::vec![self.command()?];
        while self.is_op("|") {
            self.pos += 1;
            self.skip_newlines();
            if self.peek().is_none() {
                return Err(ParseError::Incomplete);
            }
            nodes.push(self.command()?);
        }
        Ok(Pipeline { nodes, negate })
    }

    fn redirect(&mut self) -> Result<Option<Redir>, ParseError> {
        let kind = match self.peek() {
            Some(Token::Op("<")) => RedirKind::In,
            Some(Token::Op(">")) => RedirKind::Out,
            Some(Token::Op(">>")) => RedirKind::Append,
            Some(Token::Op("2>")) => RedirKind::Err,
            Some(Token::Op("2>>")) => RedirKind::ErrAppend,
            Some(Token::Op("2>&1")) => {
                self.pos += 1;
                return Ok(Some(Redir { kind: RedirKind::ErrToOut, target: Vec::new() }));
            }
            Some(Token::Op(">&2")) => {
                self.pos += 1;
                return Ok(Some(Redir { kind: RedirKind::OutToErr, target: Vec::new() }));
            }
            _ => return Ok(None),
        };
        self.pos += 1;
        match self.peek() {
            Some(Token::Word(w)) => {
                let target = w.clone();
                self.pos += 1;
                Ok(Some(Redir { kind, target }))
            }
            None => Err(ParseError::Incomplete),
            _ => Err(ParseError::Syntax("expected a file name after redirection".into())),
        }
    }

    fn redirects(&mut self) -> Result<Vec<Redir>, ParseError> {
        let mut v = Vec::new();
        while let Some(r) = self.redirect()? {
            v.push(r);
        }
        Ok(v)
    }

    fn command(&mut self) -> Result<Node, ParseError> {
        let cmd = match self.peek() {
            None => return Err(ParseError::Incomplete),
            Some(Token::Op("(")) => {
                self.pos += 1;
                let list = self.list(&[")"])?;
                self.expect_op(")")?;
                Command::Subshell(list)
            }
            Some(t) => match t.plain() {
                Some("if") => self.if_clause()?,
                Some("while") | Some("until") => {
                    let until = self.peek_word() == Some("until");
                    self.pos += 1;
                    let cond = self.list(&["do"])?;
                    self.expect_word("do")?;
                    let body = self.list(&["done"])?;
                    self.expect_word("done")?;
                    Command::While { cond, body, until }
                }
                Some("for") => self.for_clause()?,
                Some("{") => {
                    self.pos += 1;
                    let list = self.list(&["}"])?;
                    self.expect_word("}")?;
                    Command::Group(list)
                }
                Some(w) if RESERVED.contains(&w) => {
                    return Err(ParseError::Syntax(alloc::format!("unexpected '{}'", w)));
                }
                Some(name)
                    if matches!(self.tokens.get(self.pos + 1), Some(Token::Op("(")))
                        && matches!(self.tokens.get(self.pos + 2), Some(Token::Op(")"))) =>
                {
                    let name = String::from(name);
                    self.pos += 3;
                    self.skip_newlines();
                    let body = self.command()?;
                    return Ok(Node { cmd: Command::Function { name, body: Box::new(body) }, redirs: Vec::new() });
                }
                _ => return self.simple(),
            },
        };
        let redirs = self.redirects()?;
        Ok(Node { cmd, redirs })
    }

    fn if_clause(&mut self) -> Result<Command, ParseError> {
        self.pos += 1; // if
        let mut branches = Vec::new();
        let mut otherwise = None;
        loop {
            let cond = self.list(&["then"])?;
            self.expect_word("then")?;
            let body = self.list(&["elif", "else", "fi"])?;
            branches.push((cond, body));
            self.skip_newlines();
            match self.peek_word() {
                Some("elif") => {
                    self.pos += 1;
                }
                Some("else") => {
                    self.pos += 1;
                    otherwise = Some(self.list(&["fi"])?);
                    self.expect_word("fi")?;
                    break;
                }
                Some("fi") => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(ParseError::Incomplete),
            }
        }
        Ok(Command::If { branches, otherwise })
    }

    fn for_clause(&mut self) -> Result<Command, ParseError> {
        self.pos += 1; // for
        let var = match self.peek_word() {
            Some(v) => String::from(v),
            None if self.peek().is_none() => return Err(ParseError::Incomplete),
            None => return Err(ParseError::Syntax("expected a variable name after 'for'".into())),
        };
        self.pos += 1;
        self.skip_newlines();
        let items = if self.peek_word() == Some("in") {
            self.pos += 1;
            let mut items = Vec::new();
            while let Some(Token::Word(w)) = self.peek() {
                items.push(w.clone());
                self.pos += 1;
            }
            if self.is_op(";") {
                self.pos += 1;
            }
            Some(items)
        } else {
            if self.is_op(";") {
                self.pos += 1;
            }
            None
        };
        self.expect_word("do")?;
        let body = self.list(&["done"])?;
        self.expect_word("done")?;
        Ok(Command::For { var, items, body })
    }

    fn simple(&mut self) -> Result<Node, ParseError> {
        let mut assigns = Vec::new();
        let mut words = Vec::new();
        let mut redirs = Vec::new();
        loop {
            if let Some(r) = self.redirect()? {
                redirs.push(r);
                continue;
            }
            let Some(Token::Word(w)) = self.peek() else { break };
            let w = w.clone();
            // NAME=value before the command name is an assignment.
            if words.is_empty() {
                if let Some(Part::Lit(first)) = w.first() {
                    if let Some(eq) = first.find('=') {
                        let name = &first[..eq];
                        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                            let mut value: Word = Vec::new();
                            if eq + 1 < first.len() {
                                value.push(Part::Lit(String::from(&first[eq + 1..])));
                            }
                            value.extend(w[1..].iter().cloned());
                            assigns.push((String::from(name), value));
                            self.pos += 1;
                            continue;
                        }
                    }
                }
            }
            words.push(w);
            self.pos += 1;
        }
        if words.is_empty() && assigns.is_empty() && redirs.is_empty() {
            return match self.peek() {
                None => Err(ParseError::Incomplete),
                Some(Token::Op(op)) => Err(ParseError::Syntax(alloc::format!("syntax error near '{}'", op))),
                _ => Err(ParseError::Syntax("syntax error".into())),
            };
        }
        Ok(Node { cmd: Command::Simple { assigns, words }, redirs })
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<List, ParseError> {
    let mut p = Parser { tokens, pos: 0 };
    let list = p.list(&[])?;
    if p.pos < p.tokens.len() {
        return Err(ParseError::Syntax("unexpected token".into()));
    }
    Ok(list)
}
