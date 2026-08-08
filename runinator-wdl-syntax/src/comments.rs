// lossless comment capture. the pest grammar treats `COMMENT` as silent trivia, so comments never
// reach the parse tree. to preserve them across a parse -> format round-trip we lex them separately
// (byte-span accurate, string/raw-block aware) and attach each to the nearest ast anchor as either a
// leading (own-line, rendered above) or trailing (same-line, rendered after) comment.

use crate::ast::*;
use crate::errors::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    /// `// ...` to end of line.
    Line,
    /// `/* ... */`, possibly spanning multiple lines.
    Block,
}

/// a single source comment with its byte span and verbatim text (delimiters included).
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub kind: CommentKind,
    /// the exact source text, e.g. `// note` or `/* note */`.
    pub text: String,
    pub span: Span,
    /// true when only whitespace precedes the comment on its line (so it renders on its own line).
    pub own_line: bool,
}

/// the comments bound to one ast anchor: `leading` render on their own lines above it, `trailing`
/// renders as a suffix on the anchor's last line, and `dangling` render on their own lines after it
/// (used for comments trapped after the last statement of a block, before its closing brace).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentSet {
    pub leading: Vec<Comment>,
    pub trailing: Option<Comment>,
    pub dangling: Vec<Comment>,
}

impl CommentSet {
    pub fn is_empty(&self) -> bool {
        self.leading.is_empty() && self.trailing.is_none() && self.dangling.is_empty()
    }
}

/// lex every comment out of `src`, skipping comment-like byte sequences that live inside string
/// literals (including `${...}` interpolation) and raw ` ``` ` blocks. returns them in source order.
pub fn extract_comments(src: &str) -> Vec<Comment> {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'`' if starts_with(bytes, i, b"```") => i = skip_raw_block(bytes, i),
            b'"' => i = skip_string(bytes, i),
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                let start = i;
                let mut j = i + 2;
                while j < len && bytes[j] != b'\n' {
                    j += 1;
                }
                // keep the newline out of the comment; drop a trailing carriage return too.
                let mut end = j;
                if end > start && bytes[end - 1] == b'\r' {
                    end -= 1;
                }
                out.push(make_comment(src, start, end, CommentKind::Line));
                i = j;
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'*' => {
                let start = i;
                let end = skip_block_comment(bytes, i);
                out.push(make_comment(src, start, end, CommentKind::Block));
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

fn make_comment(src: &str, start: usize, end: usize, kind: CommentKind) -> Comment {
    Comment {
        kind,
        text: src[start..end].to_string(),
        span: Span::new(start, end),
        own_line: is_own_line(src.as_bytes(), start),
    }
}

fn is_own_line(bytes: &[u8], start: usize) -> bool {
    let mut k = start;
    while k > 0 {
        match bytes[k - 1] {
            b'\n' => return true,
            b' ' | b'\t' | b'\r' => k -= 1,
            _ => return false,
        }
    }
    true
}

fn starts_with(bytes: &[u8], i: usize, needle: &[u8]) -> bool {
    bytes.len() >= i + needle.len() && &bytes[i..i + needle.len()] == needle
}

// advance past a `"..."` string literal, honoring `\` escapes and `${...}` interpolation.
fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    i += 1; // opening quote.
    while i < len {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            b'$' if i + 1 < len && bytes[i + 1] == b'{' => i = skip_interpolation(bytes, i + 2),
            _ => i += 1,
        }
    }
    len
}

// advance past a `${ ... }` interpolation body (brace depth already 1), handling nested strings,
// raw blocks, and comments so a `}` inside them does not close the interpolation early.
fn skip_interpolation(bytes: &[u8], mut i: usize) -> usize {
    let len = bytes.len();
    let mut depth = 1usize;
    while i < len && depth > 0 {
        match bytes[i] {
            b'"' => i = skip_string(bytes, i),
            b'`' if starts_with(bytes, i, b"```") => i = skip_raw_block(bytes, i),
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'*' => i = skip_block_comment(bytes, i),
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

// advance past a ` ``` ... ``` ` raw block, returning the index just after the closing fence.
fn skip_raw_block(bytes: &[u8], i: usize) -> usize {
    let len = bytes.len();
    let mut j = i + 3;
    while j + 3 <= len {
        if &bytes[j..j + 3] == b"```" {
            return j + 3;
        }
        j += 1;
    }
    len
}

// advance past a `/* ... */` block comment, returning the index just after the closing `*/`.
fn skip_block_comment(bytes: &[u8], i: usize) -> usize {
    let len = bytes.len();
    let mut j = i + 2;
    while j + 2 <= len {
        if bytes[j] == b'*' && bytes[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    len
}

/// lex `src`'s comments and attach them to the document's anchors in place. call after the ast is
/// built; the formatter reads the attached comments back out.
pub fn attach_comments(document: &mut Document, src: &str) {
    let comments = extract_comments(src);
    if comments.is_empty() {
        return;
    }
    let mut cursor = Cursor { comments, i: 0 };
    attach_document(document, &mut cursor, src);
}

struct Cursor {
    comments: Vec<Comment>,
    i: usize,
}

impl Cursor {
    fn peek(&self) -> Option<&Comment> {
        self.comments.get(self.i)
    }

    fn take(&mut self) -> Comment {
        let comment = self.comments[self.i].clone();
        self.i += 1;
        comment
    }
}

// consume every comment starting before `pos` into `dst`.
fn take_leading(dst: &mut Vec<Comment>, cursor: &mut Cursor, pos: usize) {
    while let Some(comment) = cursor.peek() {
        if comment.span.start >= pos {
            break;
        }
        dst.push(cursor.take());
    }
}

// claim the immediately-next comment as trailing when it is a same-line (non-own-line) comment that
// falls before `bound`. pest rule spans include trailing trivia, so we rely on `own_line` and the
// next anchor's start rather than a rule's `span.end`, which is unreliable.
fn take_trailing(dst: &mut Option<Comment>, cursor: &mut Cursor, bound: usize) {
    if let Some(comment) = cursor.peek() {
        if comment.span.start < bound && !comment.own_line {
            *dst = Some(cursor.take());
        }
    }
}

// a commentable anchor. anchors are processed in source order; each claims own-line comments that
// precede it as leading and a following same-line comment as trailing. `bound` is the next anchor's
// start (or the container end), used as the reliable right edge in place of pest's fuzzy `span.end`.
enum Anchor<'a> {
    Leaf {
        start: usize,
        set: &'a mut CommentSet,
    },
    /// a `params { ... }` field group; `start` is the first field's start. its own line has no comment
    /// slot, so it only recurses into its fields.
    Params {
        start: usize,
        fields: &'a mut Vec<TypeField>,
    },
    /// a header `type <Name>` declaration, recursing into its struct fields when it declares one.
    TypeDecl(&'a mut TypeDecl),
    Stmt(&'a mut Stmt),
    Workflow(&'a mut Workflow),
}

impl Anchor<'_> {
    fn start(&self) -> usize {
        match self {
            Anchor::Leaf { start, .. } => *start,
            Anchor::Params { start, .. } => *start,
            Anchor::TypeDecl(decl) => decl.span.start,
            Anchor::Stmt(stmt) => stmt.span.start,
            Anchor::Workflow(workflow) => workflow.span.start,
        }
    }

    fn process(&mut self, cursor: &mut Cursor, bound: usize, src: &str) {
        match self {
            Anchor::Leaf { start, set } => {
                take_leading(&mut set.leading, cursor, *start);
                take_trailing(&mut set.trailing, cursor, bound);
            }
            Anchor::Params { fields, .. } => attach_struct_fields(fields, cursor, src),
            Anchor::TypeDecl(decl) => {
                take_leading(&mut decl.comments.leading, cursor, decl.span.start);
                if let TypeExpr::Struct { fields, .. } = &mut decl.ty {
                    attach_struct_fields(fields, cursor, src);
                }
                take_trailing(&mut decl.comments.trailing, cursor, bound);
            }
            Anchor::Stmt(stmt) => {
                take_leading(&mut stmt.comments.leading, cursor, stmt.span.start);
                attach_stmt_interior(stmt, cursor, bound, src);
                take_trailing(&mut stmt.comments.trailing, cursor, bound);
            }
            Anchor::Workflow(workflow) => {
                take_leading(&mut workflow.leading_comments, cursor, workflow.span.start);
                attach_workflow(workflow, cursor, src);
            }
        }
    }
}

// attach comments to a struct's fields (`params` fields or `type` struct fields). each field claims
// its leading/trailing comments and recurses into nested struct fields; comments after the last field
// (before the closing brace, located by matching source braces) become that field's dangling comments.
fn attach_struct_fields(fields: &mut [TypeField], cursor: &mut Cursor, src: &str) {
    let Some(first_start) = fields.first().map(|field| field.span.start) else {
        return;
    };
    let struct_end = block_close(src, first_start);
    let starts: Vec<usize> = fields.iter().map(|field| field.span.start).collect();
    for index in 0..fields.len() {
        let bound = starts.get(index + 1).copied().unwrap_or(struct_end);
        let field = &mut fields[index];
        take_leading(&mut field.comments.leading, cursor, field.span.start);
        if let TypeExpr::Struct { fields: nested, .. } = &mut field.ty {
            attach_struct_fields(nested, cursor, src);
        }
        take_trailing(&mut field.comments.trailing, cursor, bound);
    }
    if let Some(last) = fields.last_mut() {
        take_leading(&mut last.comments.dangling, cursor, struct_end);
    }
}

// run a source-ordered list of sibling anchors against the cursor. each anchor's right edge is the
// next anchor's start, so an anchor never claims comments that belong to a later sibling.
fn process_anchors(mut anchors: Vec<Anchor>, cursor: &mut Cursor, container_end: usize, src: &str) {
    anchors.sort_by_key(Anchor::start);
    let starts: Vec<usize> = anchors.iter().map(Anchor::start).collect();
    for index in 0..anchors.len() {
        let bound = starts.get(index + 1).copied().unwrap_or(container_end);
        anchors[index].process(cursor, bound, src);
    }
}

// top-level items (functions and workflows) in source order; leftovers after the last item become
// the document's trailing comments.
fn attach_document(document: &mut Document, cursor: &mut Cursor, src: &str) {
    {
        let mut tops: Vec<Anchor> = Vec::new();
        for function in &mut document.functions {
            tops.push(Anchor::Leaf {
                start: function.span.start,
                set: &mut function.comments,
            });
        }
        for workflow in &mut document.workflows {
            tops.push(Anchor::Workflow(workflow));
        }
        process_anchors(tops, cursor, usize::MAX, src);
    }
    take_leading(&mut document.trailing_comments, cursor, usize::MAX);
}

// header declarations and body statements share one source-ordered anchor list because the grammar
// lets them interleave. comments after the last anchor (before the closing brace) become the
// workflow's dangling comments.
fn attach_workflow(workflow: &mut Workflow, cursor: &mut Cursor, src: &str) {
    let workflow_end = workflow.span.end;
    {
        let mut anchors: Vec<Anchor> = Vec::new();
        // params fields anchor at the first field so their comments interleave with the headers.
        if let Some(TypeExpr::Struct { fields, .. }) = &mut workflow.input {
            if let Some(start) = fields.first().map(|field| field.span.start) {
                anchors.push(Anchor::Params { start, fields });
            }
        }
        for import in &mut workflow.imports {
            anchors.push(Anchor::Leaf {
                start: import.span.start,
                set: &mut import.comments,
            });
        }
        for trigger in &mut workflow.triggers {
            anchors.push(Anchor::Leaf {
                start: trigger.span.start,
                set: &mut trigger.comments,
            });
        }
        for alias in &mut workflow.aliases {
            anchors.push(Anchor::Leaf {
                start: alias.span.start,
                set: &mut alias.comments,
            });
        }
        for decl in &mut workflow.type_decls {
            anchors.push(Anchor::TypeDecl(decl));
        }
        for stmt in &mut workflow.body {
            anchors.push(Anchor::Stmt(stmt));
        }
        process_anchors(anchors, cursor, workflow_end, src);
    }
    take_leading(&mut workflow.dangling_comments, cursor, workflow_end);
}

// process a block of statements, then park any comments trapped after the last statement (before the
// block's closing brace) on that statement as dangling comments so they render inside the block.
fn attach_block(block: &mut [Stmt], cursor: &mut Cursor, block_end: usize, src: &str) {
    let anchors = block.iter_mut().map(Anchor::Stmt).collect();
    process_anchors(anchors, cursor, block_end, src);
    if let Some(last) = block.last_mut() {
        take_leading(&mut last.comments.dangling, cursor, block_end);
    }
}

// recurse into a statement's nested blocks (if/for/while/map/match/parallel/race/try). each block is
// bounded by its real closing brace (found by matching source braces), so a comment at the end of one
// branch stays in that branch instead of leaking into the next branch's leading comments.
fn attach_stmt_interior(stmt: &mut Stmt, cursor: &mut Cursor, stmt_end: usize, src: &str) {
    let blocks = nested_blocks_mut(&mut stmt.kind);
    if blocks.is_empty() {
        return;
    }
    for block in blocks {
        // an empty block has no anchor and no place to hold comments; bound the recursion by the
        // statement's right edge so those comments migrate to the following anchor rather than lodging
        // in a phantom branch.
        let block_end = block
            .first()
            .map(|first| block_close(src, first.span.start))
            .unwrap_or(stmt_end);
        attach_block(block, cursor, block_end, src);
    }
}

// find the byte index of the `}` that closes a block whose first statement begins at `start`. scans
// forward from inside the block (depth 1), skipping braces that live in strings, raw blocks, and
// comments. returns the source length if the block is unterminated.
fn block_close(src: &str, start: usize) -> usize {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut i = start;
    let mut depth = 1usize;
    while i < len {
        match bytes[i] {
            b'`' if starts_with(bytes, i, b"```") => i = skip_raw_block(bytes, i),
            b'"' => i = skip_string(bytes, i),
            b'/' if i + 1 < len && bytes[i + 1] == b'/' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'*' => i = skip_block_comment(bytes, i),
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    len
}

fn nested_blocks_mut(kind: &mut StmtKind) -> Vec<&mut Block> {
    match kind {
        StmtKind::If(stmt) => {
            let mut blocks: Vec<&mut Block> = stmt.arms.iter_mut().map(|(_, body)| body).collect();
            if let Some(else_block) = &mut stmt.else_block {
                blocks.push(else_block);
            }
            blocks
        }
        StmtKind::For(stmt) => vec![&mut stmt.body],
        StmtKind::While(stmt) => vec![&mut stmt.body],
        StmtKind::Map(stmt) => vec![&mut stmt.body],
        StmtKind::Match(stmt) => {
            let mut blocks: Vec<&mut Block> =
                stmt.arms.iter_mut().map(|arm| &mut arm.body).collect();
            if let Some(default) = &mut stmt.default {
                blocks.push(default);
            }
            blocks
        }
        StmtKind::Parallel(stmt) => stmt.branches.iter_mut().collect(),
        StmtKind::Race(stmt) => stmt.branches.iter_mut().collect(),
        StmtKind::Try(stmt) => {
            let mut blocks = vec![&mut stmt.body];
            if let Some(catch) = &mut stmt.catch {
                blocks.push(catch);
            }
            if let Some(finally) = &mut stmt.finally {
                blocks.push(finally);
            }
            blocks
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
