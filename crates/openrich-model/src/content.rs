//! Content expressions (`"paragraph block*"`) compiled to a DFA, as in
//! ProseMirror's `content.ts`.

use std::collections::HashMap;
use std::fmt;

use crate::fragment::Fragment;
use crate::mark::MarkSet;
use crate::schema::{NodeType, NodeTypeData, Schema};

#[derive(Debug, Clone)]
pub(crate) struct MatchState {
    pub valid_end: bool,
    pub next: Vec<(u16, u32)>,
}

impl MatchState {
    pub fn empty() -> Self {
        Self {
            valid_end: true,
            next: Vec::new(),
        }
    }
}

/// A state in a node type's content automaton.
#[derive(Clone)]
pub struct ContentMatch {
    pub(crate) schema: Schema,
    pub(crate) state: u32,
}

impl PartialEq for ContentMatch {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state && self.schema == other.schema
    }
}

impl Eq for ContentMatch {}

impl fmt::Debug for ContentMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = self
            .data()
            .next
            .iter()
            .map(|(ty, next)| format!("{}->{next}", self.schema.0.nodes[*ty as usize].name))
            .collect();
        write!(
            f,
            "ContentMatch({}{}, [{}])",
            self.state,
            if self.valid_end() { "*" } else { "" },
            names.join(", ")
        )
    }
}

impl ContentMatch {
    fn data(&self) -> &MatchState {
        &self.schema.0.matches[self.state as usize]
    }

    fn node_type(&self, id: u16) -> NodeType {
        NodeType {
            schema: self.schema.clone(),
            id,
        }
    }

    fn at(&self, state: u32) -> ContentMatch {
        self.schema.content_match(state)
    }

    /// True when the content may end here.
    pub fn valid_end(&self) -> bool {
        self.data().valid_end
    }

    pub fn match_type(&self, ty: &NodeType) -> Option<ContentMatch> {
        self.data()
            .next
            .iter()
            .find(|(id, _)| *id == ty.id)
            .map(|(_, next)| self.at(*next))
    }

    /// Matches `fragment.child(start..end)` and returns the resulting state.
    pub fn match_fragment(
        &self,
        fragment: &Fragment,
        start: usize,
        end: usize,
    ) -> Option<ContentMatch> {
        let mut current = self.clone();
        for index in start..end {
            current = current.match_type(fragment.child(index).node_type())?;
        }
        Some(current)
    }

    pub fn inline_content(&self) -> bool {
        self.data()
            .next
            .first()
            .is_some_and(|(id, _)| !self.schema.0.nodes[*id as usize].is_block)
    }

    /// The first type that can be generated here.
    pub fn default_type(&self) -> Option<NodeType> {
        self.data()
            .next
            .iter()
            .map(|(id, _)| self.node_type(*id))
            .find(|ty| !(ty.is_text() || ty.has_required_attrs()))
    }

    pub fn compatible(&self, other: &ContentMatch) -> bool {
        self.data()
            .next
            .iter()
            .any(|(a, _)| other.data().next.iter().any(|(b, _)| a == b))
    }

    pub fn edge_count(&self) -> usize {
        self.data().next.len()
    }

    pub fn edge(&self, index: usize) -> Option<(NodeType, ContentMatch)> {
        self.data()
            .next
            .get(index)
            .map(|(id, next)| (self.node_type(*id), self.at(*next)))
    }

    /// Finds generatable nodes that, inserted before `after` (from
    /// `start_index`), make the content match; with `to_end` the result must
    /// also be able to end there.
    pub fn fill_before(
        &self,
        after: &Fragment,
        to_end: bool,
        start_index: usize,
    ) -> Option<Fragment> {
        let mut seen = vec![self.state];
        self.search_fill(after, to_end, start_index, &mut seen, &mut Vec::new())
    }

    fn search_fill(
        &self,
        after: &Fragment,
        to_end: bool,
        start_index: usize,
        seen: &mut Vec<u32>,
        types: &mut Vec<NodeType>,
    ) -> Option<Fragment> {
        if let Some(finished) = self.match_fragment(after, start_index, after.child_count())
            && (!to_end || finished.valid_end())
        {
            let nodes = types
                .iter()
                .map(|ty| ty.create_and_fill(None, Fragment::empty(), MarkSet::none()))
                .collect::<Option<Vec<_>>>()?;
            return Some(Fragment::from_array(nodes));
        }
        for (id, next) in self.data().next.clone() {
            let ty = self.node_type(id);
            if !(ty.is_text() || ty.has_required_attrs()) && !seen.contains(&next) {
                seen.push(next);
                types.push(ty);
                if let Some(found) =
                    self.at(next)
                        .search_fill(after, to_end, start_index, seen, types)
                {
                    return Some(found);
                }
                types.pop();
            }
        }
        None
    }

    /// The wrapper types (outermost first) needed to place `target` here.
    pub fn find_wrapping(&self, target: &NodeType) -> Option<Vec<NodeType>> {
        struct Active {
            matched: ContentMatch,
            ty: Option<NodeType>,
            via: Option<usize>,
        }
        let mut seen = vec![false; self.schema.0.nodes.len()];
        let mut active = vec![Active {
            matched: self.clone(),
            ty: None,
            via: None,
        }];
        let mut cursor = 0;
        while cursor < active.len() {
            let current = &active[cursor];
            if current.matched.match_type(target).is_some() {
                let mut result = Vec::new();
                let mut walk = Some(cursor);
                while let Some(index) = walk {
                    if let Some(ty) = &active[index].ty {
                        result.push(ty.clone());
                    }
                    walk = active[index].via;
                }
                result.reverse();
                return Some(result);
            }
            let needs_end = current.ty.is_some();
            let edges = current.matched.data().next.clone();
            for (id, next) in edges {
                let ty = self.node_type(id);
                if !ty.is_leaf()
                    && !ty.has_required_attrs()
                    && !seen[id as usize]
                    && (!needs_end || self.at(next).valid_end())
                {
                    seen[id as usize] = true;
                    active.push(Active {
                        matched: ty.content_match(),
                        ty: Some(ty),
                        via: Some(cursor),
                    });
                }
            }
            cursor += 1;
        }
        None
    }
}

// ---- Expression compiler ---------------------------------------------------

enum Expr {
    Choice(Vec<Expr>),
    Seq(Vec<Expr>),
    Plus(Box<Expr>),
    Star(Box<Expr>),
    Opt(Box<Expr>),
    Range {
        min: usize,
        max: Option<usize>,
        expr: Box<Expr>,
    },
    Name(u16),
}

struct TokenStream<'a> {
    tokens: Vec<String>,
    pos: usize,
    nodes: &'a [NodeTypeData],
    names: &'a HashMap<String, u16>,
    inline: Option<bool>,
}

impl TokenStream<'_> {
    fn next(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.next() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

fn tokenize(expr: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
        } else if ch.is_alphanumeric() || ch == '_' {
            let mut word = String::new();
            while let Some(&ch) = chars.peek() {
                if !(ch.is_alphanumeric() || ch == '_') {
                    break;
                }
                word.push(ch);
                chars.next();
            }
            tokens.push(word);
        } else {
            tokens.push(ch.to_string());
            chars.next();
        }
    }
    tokens
}

fn parse_expr(stream: &mut TokenStream) -> Result<Expr, String> {
    let mut exprs = Vec::new();
    loop {
        exprs.push(parse_seq(stream)?);
        if !stream.eat("|") {
            break;
        }
    }
    Ok(if exprs.len() == 1 {
        exprs.remove(0)
    } else {
        Expr::Choice(exprs)
    })
}

fn parse_seq(stream: &mut TokenStream) -> Result<Expr, String> {
    let mut exprs = Vec::new();
    loop {
        exprs.push(parse_subscript(stream)?);
        match stream.next() {
            Some(")") | Some("|") | None => break,
            Some(_) => {}
        }
    }
    Ok(if exprs.len() == 1 {
        exprs.remove(0)
    } else {
        Expr::Seq(exprs)
    })
}

fn parse_subscript(stream: &mut TokenStream) -> Result<Expr, String> {
    let mut expr = parse_atom(stream)?;
    loop {
        if stream.eat("+") {
            expr = Expr::Plus(Box::new(expr));
        } else if stream.eat("*") {
            expr = Expr::Star(Box::new(expr));
        } else if stream.eat("?") {
            expr = Expr::Opt(Box::new(expr));
        } else if stream.eat("{") {
            expr = parse_range(stream, expr)?;
        } else {
            return Ok(expr);
        }
    }
}

fn parse_num(stream: &mut TokenStream) -> Result<usize, String> {
    let token = stream.next().unwrap_or("").to_owned();
    let value = token
        .parse::<usize>()
        .map_err(|_| format!("expected number, got `{token}`"))?;
    stream.pos += 1;
    Ok(value)
}

fn parse_range(stream: &mut TokenStream, expr: Expr) -> Result<Expr, String> {
    let min = parse_num(stream)?;
    let mut max = Some(min);
    if stream.eat(",") {
        max = if stream.next() != Some("}") {
            Some(parse_num(stream)?)
        } else {
            None
        };
    }
    if !stream.eat("}") {
        return Err("unclosed braced range".into());
    }
    Ok(Expr::Range {
        min,
        max,
        expr: Box::new(expr),
    })
}

fn resolve_name(stream: &TokenStream, name: &str) -> Result<Vec<u16>, String> {
    if let Some(id) = stream.names.get(name) {
        return Ok(vec![*id]);
    }
    let found: Vec<u16> = stream
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.groups.iter().any(|group| group == name))
        .map(|(id, _)| id as u16)
        .collect();
    if found.is_empty() {
        Err(format!("no node type or group `{name}` found"))
    } else {
        Ok(found)
    }
}

fn parse_atom(stream: &mut TokenStream) -> Result<Expr, String> {
    if stream.eat("(") {
        let expr = parse_expr(stream)?;
        if !stream.eat(")") {
            return Err("missing closing paren".into());
        }
        return Ok(expr);
    }
    let Some(token) = stream.next().map(str::to_owned) else {
        return Err("unexpected end of expression".into());
    };
    if !token.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
        return Err(format!("unexpected token `{token}`"));
    }
    let mut exprs = Vec::new();
    for id in resolve_name(stream, &token)? {
        let inline = !stream.nodes[id as usize].is_block;
        match stream.inline {
            None => stream.inline = Some(inline),
            Some(previous) if previous != inline => {
                return Err("mixing inline and block content".into());
            }
            Some(_) => {}
        }
        exprs.push(Expr::Name(id));
    }
    stream.pos += 1;
    Ok(if exprs.len() == 1 {
        exprs.remove(0)
    } else {
        Expr::Choice(exprs)
    })
}

struct NfaEdge {
    term: Option<u16>,
    to: Option<usize>,
}

struct Nfa {
    nodes: Vec<Vec<usize>>,
    edges: Vec<NfaEdge>,
}

impl Nfa {
    fn node(&mut self) -> usize {
        self.nodes.push(Vec::new());
        self.nodes.len() - 1
    }

    fn edge(&mut self, from: usize, to: Option<usize>, term: Option<u16>) -> usize {
        self.edges.push(NfaEdge { term, to });
        let id = self.edges.len() - 1;
        self.nodes[from].push(id);
        id
    }

    fn connect(&mut self, edges: &[usize], to: usize) {
        for edge in edges {
            self.edges[*edge].to = Some(to);
        }
    }

    fn compile(&mut self, expr: &Expr, from: usize) -> Vec<usize> {
        match expr {
            Expr::Choice(exprs) => exprs
                .iter()
                .flat_map(|expr| self.compile(expr, from))
                .collect(),
            Expr::Seq(exprs) => {
                let mut from = from;
                for (index, expr) in exprs.iter().enumerate() {
                    let next = self.compile(expr, from);
                    if index == exprs.len() - 1 {
                        return next;
                    }
                    from = self.node();
                    self.connect(&next, from);
                }
                Vec::new()
            }
            Expr::Star(expr) => {
                let looped = self.node();
                self.edge(from, Some(looped), None);
                let inner = self.compile(expr, looped);
                self.connect(&inner, looped);
                vec![self.edge(looped, None, None)]
            }
            Expr::Plus(expr) => {
                let looped = self.node();
                let first = self.compile(expr, from);
                self.connect(&first, looped);
                let again = self.compile(expr, looped);
                self.connect(&again, looped);
                vec![self.edge(looped, None, None)]
            }
            Expr::Opt(expr) => {
                let mut out = vec![self.edge(from, None, None)];
                out.extend(self.compile(expr, from));
                out
            }
            Expr::Range { min, max, expr } => {
                let mut current = from;
                for _ in 0..*min {
                    let next = self.node();
                    let edges = self.compile(expr, current);
                    self.connect(&edges, next);
                    current = next;
                }
                match max {
                    None => {
                        let edges = self.compile(expr, current);
                        self.connect(&edges, current);
                    }
                    Some(max) => {
                        for _ in *min..*max {
                            let next = self.node();
                            self.edge(current, Some(next), None);
                            let edges = self.compile(expr, current);
                            self.connect(&edges, next);
                            current = next;
                        }
                    }
                }
                vec![self.edge(current, None, None)]
            }
            Expr::Name(id) => vec![self.edge(from, None, Some(*id))],
        }
    }

    /// NFA states reachable from `node` through empty edges, sorted descending.
    fn null_from(&self, node: usize) -> Vec<usize> {
        let mut result = Vec::new();
        self.scan(node, &mut result);
        result.sort_unstable_by(|a, b| b.cmp(a));
        result
    }

    fn scan(&self, node: usize, result: &mut Vec<usize>) {
        let edges = &self.nodes[node];
        if edges.len() == 1 && self.edges[edges[0]].term.is_none() {
            if let Some(to) = self.edges[edges[0]].to {
                self.scan(to, result);
            }
            return;
        }
        result.push(node);
        for edge in edges {
            let edge = &self.edges[*edge];
            if edge.term.is_none()
                && let Some(to) = edge.to
                && !result.contains(&to)
            {
                self.scan(to, result);
            }
        }
    }
}

fn explore(
    nfa: &Nfa,
    states: Vec<usize>,
    labeled: &mut HashMap<Vec<usize>, u32>,
    matches: &mut Vec<MatchState>,
) -> u32 {
    let mut out: Vec<(u16, Vec<usize>)> = Vec::new();
    for node in &states {
        for edge in &nfa.nodes[*node] {
            let edge = &nfa.edges[*edge];
            let (Some(term), Some(to)) = (edge.term, edge.to) else {
                continue;
            };
            let index = match out.iter().position(|(ty, _)| *ty == term) {
                Some(index) => index,
                None => {
                    out.push((term, Vec::new()));
                    out.len() - 1
                }
            };
            for reached in nfa.null_from(to) {
                if !out[index].1.contains(&reached) {
                    out[index].1.push(reached);
                }
            }
        }
    }
    let id = matches.len() as u32;
    matches.push(MatchState {
        valid_end: states.contains(&(nfa.nodes.len() - 1)),
        next: Vec::new(),
    });
    labeled.insert(states, id);
    for (term, mut targets) in out {
        targets.sort_unstable_by(|a, b| b.cmp(a));
        let next = match labeled.get(&targets) {
            Some(next) => *next,
            None => explore(nfa, targets, labeled, matches),
        };
        matches[id as usize].next.push((term, next));
    }
    id
}

/// Compiles `expr` into `matches` and returns its start state.
pub(crate) fn compile(
    expr: &str,
    nodes: &[NodeTypeData],
    names: &HashMap<String, u16>,
    matches: &mut Vec<MatchState>,
) -> Result<u32, String> {
    let mut stream = TokenStream {
        tokens: tokenize(expr),
        pos: 0,
        nodes,
        names,
        inline: None,
    };
    if stream.next().is_none() {
        return Ok(0);
    }
    let parsed = parse_expr(&mut stream)?;
    if let Some(token) = stream.next() {
        return Err(format!("unexpected trailing text `{token}`"));
    }
    let mut nfa = Nfa {
        nodes: vec![Vec::new()],
        edges: Vec::new(),
    };
    let edges = nfa.compile(&parsed, 0);
    let end = nfa.node();
    nfa.connect(&edges, end);
    let mut labeled = HashMap::new();
    let start = explore(&nfa, nfa.null_from(0), &mut labeled, matches);

    let mut work = vec![start];
    let mut index = 0;
    while index < work.len() {
        let state = &matches[work[index] as usize];
        let mut dead = !state.valid_end;
        let mut names = Vec::new();
        for (ty, next) in &state.next {
            let node = &nodes[*ty as usize];
            names.push(node.name.clone());
            if dead && !(node.is_text || node.default_attrs.is_none()) {
                dead = false;
            }
            if !work.contains(next) {
                work.push(*next);
            }
        }
        if dead {
            return Err(format!(
                "only non-generatable nodes ({}) in a required position",
                names.join(", ")
            ));
        }
        index += 1;
    }
    Ok(start)
}
