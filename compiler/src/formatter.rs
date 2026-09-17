use crate::{ast::*, diagnostic::Diagnostic, parser::parse, span::Span};

#[derive(Debug, Clone)]
struct Comment {
    span: Span,
    text: String,
}

/// Format a single Skuld source file. Returns the formatted text, or the
/// diagnostics the parser produced when the source is invalid.
pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let parsed = parse(source);
    let program = parsed.program.ok_or(parsed.diagnostics.clone())?;
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    let comments = extract_comments(source);
    let mut formatter = Formatter::new(source, comments);
    formatter.format_program(&program);
    Ok(formatter.output)
}

/// Scan source text for `//` comments outside of string literals.
fn extract_comments(source: &str) -> Vec<Comment> {
    let mut comments = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip string literals.
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Skip char literals.
        if bytes[i] == b'\'' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\'' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            comments.push(Comment {
                span: Span::new(start, i),
                text: source[start..i].to_string(),
            });
            continue;
        }
        i += 1;
    }
    comments
}

struct Formatter<'a> {
    source: &'a str,
    comments: Vec<Comment>,
    comment_idx: usize,
    output: String,
    indent_level: usize,
    needs_indent: bool,
    /// How far into the source the output has already accounted for. A blank
    /// line is preserved by measuring the distance from the last thing
    /// emitted, so this has to be the end of that thing — code or comment —
    /// and not the end of the previous comment alone, or every comment that
    /// follows a run of code inherits the gap before it.
    covered: usize,
}

enum Decl<'a> {
    Import(&'a ImportDecl),
    Interface(&'a InterfaceDecl),
    Struct(&'a StructDecl),
    Enum(&'a EnumDecl),
    Constant(&'a ConstantDecl),
    Static(&'a StaticDecl),
    Function(&'a FunctionDecl),
    Extern(&'a ExternBlock),
}

impl Decl<'_> {
    fn span(&self) -> Span {
        match self {
            Decl::Import(d) => d.span,
            Decl::Interface(d) => d.span,
            Decl::Struct(d) => d.span,
            Decl::Enum(d) => d.span,
            Decl::Constant(d) => d.span,
            Decl::Static(d) => d.span,
            Decl::Function(d) => d.span,
            Decl::Extern(d) => d.span,
        }
    }
}

impl<'a> Formatter<'a> {
    fn new(source: &'a str, comments: Vec<Comment>) -> Self {
        Self {
            source,
            comments,
            comment_idx: 0,
            output: String::new(),
            indent_level: 0,
            needs_indent: true,
            covered: 0,
        }
    }

    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.needs_indent && text != "\n" {
            for _ in 0..self.indent_level {
                self.output.push_str("    ");
            }
            self.needs_indent = false;
        }
        self.output.push_str(text);
        if text.ends_with('\n') {
            self.needs_indent = true;
        }
    }

    fn push_line(&mut self, text: &str) {
        self.push(text);
        self.push("\n");
    }

    fn indent(&mut self) {
        self.indent_level += 1;
    }

    fn dedent(&mut self) {
        self.indent_level -= 1;
    }

    /// What comes before an item: the blank line the author left, and then any
    /// comments. A blank line is how a reader groups statements, so losing it
    /// changes the shape of every function it appears in.
    fn lead_in(&mut self, start: usize) {
        // The gap is measured to whatever comes first — a comment of this
        // item's, or the item itself.
        let first = self
            .comments
            .get(self.comment_idx)
            .map(|comment| comment.span.start)
            .filter(|comment_start| *comment_start <= start)
            .unwrap_or(start);
        if self
            .source
            .get(self.covered..first)
            .is_some_and(|gap| gap.matches('\n').count() >= 2)
            && !self.output.ends_with("{\n")
            && !self.output.ends_with("\n\n")
        {
            if self.output.ends_with('\n') {
                self.push("\n");
            } else {
                self.push("\n\n");
            }
        }
        // A comment the source kept apart from the item stays apart.
        if let Some(end) = self.emit_comments_before(start)
            && self.source[end..start].matches('\n').count() >= 2
            && !self.output.ends_with("\n\n")
        {
            self.push("\n");
        }
    }

    /// Emit any comments whose start position is before `pos`, and answer
    /// where the last of them ended.
    fn emit_comments_before(&mut self, pos: usize) -> Option<usize> {
        let mut last_end = self.covered.max(if self.comment_idx > 0 {
            self.comments[self.comment_idx - 1].span.end
        } else {
            0
        });
        let mut emitted = None;

        while self.comment_idx < self.comments.len() {
            let comment_start = self.comments[self.comment_idx].span.start;
            let comment_end = self.comments[self.comment_idx].span.end;
            if comment_start > pos {
                break;
            }
            // If there was a blank line between the previous content and this
            // comment, preserve a single blank line.
            let between = &self.source[last_end..comment_start];
            if between.matches('\n').count() >= 2
                && !self.output.ends_with("{\n")
                && !self.output.ends_with("\n\n")
            {
                if self.output.ends_with('\n') {
                    self.push("\n");
                } else {
                    self.push("\n\n");
                }
            }

            let text = self.comments[self.comment_idx].text.clone();
            self.push_line(&text);
            last_end = comment_end;
            emitted = Some(comment_end);
            self.comment_idx += 1;
        }
        self.covered = self.covered.max(pos);
        emitted
    }

    /// Emit a trailing comment on the same line, then a newline.
    fn emit_trailing_comment(&mut self, line_end_pos: usize) {
        let mut scan = line_end_pos;
        let bytes = self.source.as_bytes();
        while scan < bytes.len() && (bytes[scan] == b' ' || bytes[scan] == b'\t') {
            scan += 1;
        }

        if self.comment_idx < self.comments.len() {
            let comment_start = self.comments[self.comment_idx].span.start;
            // The comment is on the same line if there is no newline between
            // the end of the code and the comment.
            let on_same_line =
                comment_start <= scan || !self.source[line_end_pos..comment_start].contains('\n');
            if on_same_line {
                self.push(" ");
                let text = self.comments[self.comment_idx].text.clone();
                self.push(&text);
                self.covered = self.comments[self.comment_idx].span.end;
                self.comment_idx += 1;
            }
        }
        self.covered = self.covered.max(line_end_pos);
        self.push("\n");
    }

    // ── Program ──────────────────────────────────────────────────────────

    fn format_program(&mut self, program: &Program) {
        let mut decls = Vec::new();
        for import in &program.imports {
            decls.push(Decl::Import(import));
        }
        for iface in &program.interfaces {
            decls.push(Decl::Interface(iface));
        }
        for st in &program.structs {
            decls.push(Decl::Struct(st));
        }
        for en in &program.enums {
            decls.push(Decl::Enum(en));
        }
        for c in &program.constants {
            decls.push(Decl::Constant(c));
        }
        for declaration in &program.statics {
            decls.push(Decl::Static(declaration));
        }
        for func in &program.functions {
            decls.push(Decl::Function(func));
        }
        for ext in &program.externs {
            decls.push(Decl::Extern(ext));
        }
        decls.sort_by_key(|d| d.span().start);

        let mut last_was_import = false;
        let mut first = true;

        for decl in &decls {
            let is_import = matches!(decl, Decl::Import(_));

            // One blank line between declarations, but consecutive imports
            // stay together. It goes in before the comments, not after them:
            // a comment written above a declaration documents it, and a blank
            // line inserted between the two would say the opposite.
            if !first && !(last_was_import && is_import) && !self.output.ends_with("\n\n") {
                if self.output.ends_with('\n') {
                    self.push("\n");
                } else {
                    self.push("\n\n");
                }
            }

            self.lead_in(decl.span().start);

            match decl {
                Decl::Import(i) => self.format_import(i),
                Decl::Interface(i) => self.format_interface(i),
                Decl::Struct(s) => self.format_struct(s),
                Decl::Enum(e) => self.format_enum(e),
                Decl::Constant(c) => self.format_constant(c),
                Decl::Static(declaration) => self.format_static(declaration),
                Decl::Function(f) => self.format_top_function(f),
                Decl::Extern(e) => self.format_extern(e),
            }

            last_was_import = is_import;
            first = false;
        }

        self.emit_comments_before(self.source.len());

        // Ensure exactly one trailing newline.
        if !self.output.ends_with('\n') {
            self.push("\n");
        }
        while self.output.ends_with("\n\n\n") {
            self.output.pop();
        }
    }

    // ── Declarations ─────────────────────────────────────────────────────

    fn format_visibility(&mut self, vis: Visibility) {
        if let Visibility::Public = vis {
            self.push("pub ");
        }
    }

    fn format_import(&mut self, import: &ImportDecl) {
        self.push("import ");
        // Use the original string literal (with quotes) from source.
        self.push(&self.source[import.path_span.start..import.path_span.end]);
        self.emit_trailing_comment(import.span.end);
    }

    fn format_interface(&mut self, iface: &InterfaceDecl) {
        self.format_visibility(iface.visibility);
        self.push("interface ");
        self.push(&iface.name.text);
        self.push(" {");
        self.emit_trailing_comment(iface.name.span.end);
        self.indent();

        for method in &iface.methods {
            self.emit_comments_before(method.span.start);
            // Interface methods have no `func` prefix, matching class methods.
            self.push(&method.name.text);
            self.push("(");
            self.format_parameters(&method.parameters);
            self.push(")");
            if let Some(ret) = &method.return_type {
                self.push(" -> ");
                self.format_type(ret);
            }
            self.emit_trailing_comment(method.span.end);
        }

        self.dedent();
        self.emit_comments_before(iface.span.end);
        self.push("}");
        self.emit_trailing_comment(iface.span.end);
    }

    fn format_struct(&mut self, st: &StructDecl) {
        self.format_visibility(st.visibility);
        if st.layout.is_foreign() {
            self.push("extern ");
        }
        match st.kind {
            TypeDeclKind::Value => self.push("struct "),
            TypeDeclKind::Reference => self.push("class "),
            TypeDeclKind::Union => self.push("union "),
        }
        self.push(&st.name.text);
        // `packed` and `align N` sit between the name and the body, in that
        // order however they were written.
        if let Layout::Foreign { packed, align } = &st.layout {
            if *packed {
                self.push(" packed");
            }
            if let Some((value, _)) = align {
                self.push(&format!(" align {value}"));
            }
        }
        if !st.conforms.is_empty() {
            self.push(": ");
            for (i, path) in st.conforms.iter().enumerate() {
                if i > 0 {
                    self.push(", ");
                }
                self.format_path(path);
            }
        }
        self.push(" {");
        let header_end = st.conforms.last().map_or(st.name.span.end, |p| p.span.end);
        self.emit_trailing_comment(header_end);
        self.indent();

        for field in &st.fields {
            self.emit_comments_before(field.span.start);
            self.push(&field.name.text);
            self.push(": ");
            self.format_type(&field.type_ref);
            if let Some(default) = &field.default {
                self.push(" = ");
                self.format_expr(default);
            }
            self.emit_trailing_comment(field.span.end);
        }

        if !st.fields.is_empty() && !st.methods.is_empty() {
            self.push("\n");
        }

        for (i, method) in st.methods.iter().enumerate() {
            if i > 0 {
                self.push("\n");
            }
            self.emit_comments_before(method.span.start);
            // Methods have no `func` prefix and no individual visibility.
            self.format_method(method);
        }

        self.dedent();
        self.emit_comments_before(st.span.end);
        self.push("}");
        self.emit_trailing_comment(st.span.end);
    }

    /// A method inside a struct or class: no `func`, no visibility.
    fn format_method(&mut self, f: &FunctionDecl) {
        self.push(&f.name.text);
        self.push("(");
        self.format_parameters(&f.parameters);
        self.push(")");
        if let Some(ret) = &f.return_type {
            self.push(" -> ");
            self.format_type(ret);
        }
        self.push(" ");
        self.format_block_inline(&f.body);
    }

    fn format_enum(&mut self, en: &EnumDecl) {
        self.format_visibility(en.visibility);
        self.push("enum ");
        self.push(&en.name.text);
        if let Some(underlying) = &en.underlying {
            self.push(": ");
            self.format_type(underlying);
        }
        self.push(" {");
        let header_end = en
            .underlying
            .as_ref()
            .map_or(en.name.span.end, |ty| ty.span().end);
        self.emit_trailing_comment(header_end);

        self.indent();
        for variant in &en.variants {
            self.emit_comments_before(variant.span.start);
            self.push(&variant.name.text);
            if let Some(payload) = &variant.payload {
                self.push("(");
                self.format_type(payload);
                self.push(")");
            }
            if let Some(value) = &variant.value {
                self.push(" = ");
                self.format_expr(value);
            }
            self.push(",");
            self.emit_trailing_comment(variant.span.end);
        }
        self.dedent();

        self.emit_comments_before(en.span.end);
        self.push("}");
        self.emit_trailing_comment(en.span.end);
    }

    fn format_constant(&mut self, c: &ConstantDecl) {
        self.format_visibility(c.visibility);
        self.push("const ");
        self.push(&c.name.text);
        if let Some(ty) = &c.type_ref {
            self.push(": ");
            self.format_type(ty);
        }
        self.push(" = ");
        self.format_expr(&c.value);
        self.emit_trailing_comment(c.span.end);
    }

    fn format_static(&mut self, declaration: &StaticDecl) {
        self.format_visibility(declaration.visibility);
        self.push("static ");
        self.push(&declaration.name.text);
        if let Some(ty) = &declaration.type_ref {
            self.push(": ");
            self.format_type(ty);
        }
        self.push(" = ");
        self.format_expr(&declaration.value);
        self.emit_trailing_comment(declaration.span.end);
    }

    /// A top-level function: has `func` prefix and its own visibility.
    fn format_top_function(&mut self, f: &FunctionDecl) {
        self.format_visibility(f.visibility);
        self.push("func ");
        self.push(&f.name.text);
        self.push("(");
        self.format_parameters(&f.parameters);
        self.push(")");
        if let Some(ret) = &f.return_type {
            self.push(" -> ");
            self.format_type(ret);
        }
        self.push(" ");
        self.format_block_inline(&f.body);
    }

    fn format_extern(&mut self, ext: &ExternBlock) {
        self.push("unsafe extern ");
        self.push(&self.source[ext.abi_span.start..ext.abi_span.end]);
        self.push(" {");
        self.emit_trailing_comment(ext.abi_span.end);

        self.indent();
        for func in &ext.functions {
            self.emit_comments_before(func.span.start);
            self.push("func ");
            self.push(&func.name.text);
            self.push("(");
            self.format_parameters(&func.parameters);
            self.push(")");
            if let Some(ret) = &func.return_type {
                self.push(" -> ");
                self.format_type(ret);
            }
            self.emit_trailing_comment(func.span.end);
        }
        self.dedent();

        self.emit_comments_before(ext.span.end);
        self.push("}");
        self.emit_trailing_comment(ext.span.end);
    }

    fn format_parameters(&mut self, params: &[Parameter]) {
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(&param.name.text);
            self.push(": ");
            self.format_type(&param.type_ref);
        }
    }

    // ── Blocks ───────────────────────────────────────────────────────────

    fn format_block_inline(&mut self, block: &Block) {
        if block.statements.is_empty() {
            self.push("{}");
            self.emit_trailing_comment(block.span.end);
            return;
        }
        self.push("{");
        self.emit_trailing_comment(block.span.start);
        self.indent();

        for stmt in &block.statements {
            self.lead_in(stmt.span.start);
            self.format_statement(stmt);
        }

        self.dedent();
        self.emit_comments_before(block.span.end);
        self.push("}");
        self.emit_trailing_comment(block.span.end);
    }

    // ── Statements ───────────────────────────────────────────────────────

    fn format_statement(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::Variable(v) => {
                self.push(match v.mutability {
                    Mutability::Immutable => "let ",
                    Mutability::Mutable => "var ",
                });
                self.push(&v.name.text);
                if let Some(t) = &v.type_ref {
                    self.push(": ");
                    self.format_type(t);
                }
                self.push(" = ");
                self.format_expr(&v.initializer);
                if let Some(otherwise) = &v.otherwise {
                    self.push(" else ");
                    if let Some(bind) = &otherwise.binding {
                        self.push(&bind.text);
                        self.push(" ");
                    }
                    self.format_block_inline(&otherwise.block);
                    return; // trailing comment handled by block
                }
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::Constant(c) => {
                self.push("const ");
                self.push(&c.name.text);
                if let Some(t) = &c.type_ref {
                    self.push(": ");
                    self.format_type(t);
                }
                self.push(" = ");
                self.format_expr(&c.value);
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::Expression(e) => {
                self.format_expr(e);
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::Return(e) => {
                self.push("return");
                if let Some(expr) = e {
                    self.push(" ");
                    self.format_expr(expr);
                }
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::Block(b) => {
                self.format_block_inline(b);
            }
            StatementKind::Unsafe(b) => {
                self.push("unsafe ");
                self.format_block_inline(b);
            }
            StatementKind::Defer(deferred) => {
                self.push("defer ");
                self.format_statement(deferred);
            }
            StatementKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                self.push("if ");
                self.format_expr(condition);
                self.push(" ");
                self.format_block_no_trail(then_block);
                if let Some(else_stmt) = else_branch {
                    self.push(" else ");
                    self.format_else(else_stmt);
                } else {
                    self.emit_trailing_comment(then_block.span.end);
                }
            }
            StatementKind::IfLet {
                pattern,
                binding,
                value,
                then_block,
                else_branch,
            } => {
                self.push("if let ");
                // Detect whether the original source used the bare form
                // `if let name = ...` (IfLetPattern::Some without an explicit
                // `Some(...)` wrapper) by checking the source text.
                let bare = *pattern == IfLetPattern::Some && {
                    let src = &self.source[binding.span.start..binding.span.end];
                    src == binding.text
                        && (binding.span.start == 0
                            || self.source.as_bytes()[binding.span.start - 1] != b'(')
                };
                if bare {
                    self.push(&binding.text);
                } else {
                    match pattern {
                        IfLetPattern::Some => self.push("Some("),
                        IfLetPattern::Ok => self.push("Ok("),
                        IfLetPattern::Err => self.push("Err("),
                    }
                    self.push(&binding.text);
                    self.push(")");
                }
                self.push(" = ");
                self.format_expr(value);
                self.push(" ");
                self.format_block_no_trail(then_block);
                if let Some(else_stmt) = else_branch {
                    self.push(" else ");
                    self.format_else(else_stmt);
                } else {
                    self.emit_trailing_comment(then_block.span.end);
                }
            }
            StatementKind::While { condition, body } => {
                self.push("while ");
                self.format_expr(condition);
                self.push(" ");
                self.format_block_inline(body);
            }
            StatementKind::Loop { body } => {
                self.push("loop ");
                self.format_block_inline(body);
            }
            StatementKind::Break => {
                self.push("break");
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::Continue => {
                self.push("continue");
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::Match { value, arms } => {
                self.push("match ");
                self.format_expr(value);
                self.push(" {");
                self.emit_trailing_comment(value.span.end);
                self.indent();

                for arm in arms {
                    self.emit_comments_before(arm.span.start);
                    self.format_match_pattern(&arm.pattern);
                    // Detect whether the arm body is a single statement (no
                    // explicit braces in source) or a block. A synthetic block
                    // wrapping one statement has its span equal to the statement
                    // span; a real block starts with `{`.
                    let is_block = arm.body.span.start < self.source.len()
                        && self.source.as_bytes()[arm.body.span.start] == b'{';
                    if is_block {
                        self.push(": ");
                        self.format_block_inline(&arm.body);
                    } else {
                        self.push(": ");
                        // Single statement — emit inline, no braces.
                        for s in &arm.body.statements {
                            self.format_statement(s);
                        }
                    }
                }

                self.dedent();
                self.emit_comments_before(stmt.span.end);
                self.push("}");
                self.emit_trailing_comment(stmt.span.end);
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                self.push("for ");
                self.push(&variable.text);
                self.push(" in ");
                match iterable {
                    ForIterable::Range { start, end } => {
                        self.format_expr(start);
                        self.push("..");
                        self.format_expr(end);
                    }
                    ForIterable::Expr(e) => {
                        self.format_expr(e);
                    }
                }
                self.push(" ");
                self.format_block_inline(body);
            }
        }
    }

    /// Format a block without emitting the trailing comment/newline after `}`.
    /// Used for if/else chains where `} else {` must stay on one line.
    fn format_block_no_trail(&mut self, block: &Block) {
        if block.statements.is_empty() {
            self.push("{}");
            return;
        }
        self.push("{");
        self.emit_trailing_comment(block.span.start);
        self.indent();
        for stmt in &block.statements {
            self.lead_in(stmt.span.start);
            self.format_statement(stmt);
        }
        self.dedent();
        self.emit_comments_before(block.span.end);
        self.push("}");
    }

    /// Format an else branch — either `{ ... }` or `if ... { ... } else ...`.
    fn format_else(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::If { .. } | StatementKind::IfLet { .. } => {
                // else if — format the entire if statement.
                self.format_statement(stmt);
            }
            StatementKind::Block(block) => {
                self.format_block_inline(block);
            }
            _ => {
                // Should not happen in valid AST, but handle gracefully.
                self.format_statement(stmt);
            }
        }
    }

    fn format_match_pattern(&mut self, pattern: &MatchPattern) {
        match pattern {
            MatchPattern::Variant {
                enum_name,
                variant_name,
                binding,
                ..
            } => {
                if let Some(p) = enum_name {
                    self.format_path(p);
                    self.push(".");
                }
                self.push(&variant_name.text);
                if let Some(b) = binding {
                    self.push("(");
                    self.push(&b.text);
                    self.push(")");
                }
            }
            MatchPattern::Wildcard(_) => self.push("_"),
            MatchPattern::Constant(expr) => self.format_expr(expr),
            MatchPattern::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                self.format_expr(start);
                if *inclusive {
                    self.push("..=");
                } else {
                    self.push("..");
                }
                self.format_expr(end);
            }
        }
    }

    // ── Expressions ──────────────────────────────────────────────────────

    fn format_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            // Literals and interpolations: use the original source text so
            // number formatting and string escapes are preserved exactly.
            ExprKind::Literal(_) | ExprKind::Interpolation(_) => {
                self.push(&self.source[expr.span.start..expr.span.end]);
            }
            ExprKind::Identifier(name) => {
                self.push(&name.text);
            }
            ExprKind::Group(inner) => {
                self.push("(");
                self.format_expr(inner);
                self.push(")");
            }
            ExprKind::Unary { op, operand, .. } => {
                match op {
                    UnaryOp::Positive => self.push("+"),
                    UnaryOp::Negative => self.push("-"),
                    UnaryOp::Not => self.push("!"),
                    UnaryOp::BitNot => self.push("~"),
                }
                self.format_expr(operand);
            }
            ExprKind::Binary {
                left, op, right, ..
            } => {
                self.format_expr(left);
                self.push(" ");
                self.push(binary_op_str(*op));
                self.push(" ");
                self.format_expr(right);
            }
            ExprKind::Assignment {
                target, op, value, ..
            } => {
                self.format_expr(target);
                self.push(" ");
                self.push(assignment_op_str(*op));
                self.push(" ");
                self.format_expr(value);
            }
            ExprKind::Call { callee, arguments } => {
                self.format_expr(callee);
                self.push("(");
                for (i, arg) in arguments.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.format_expr(arg);
                }
                self.push(")");
            }
            ExprKind::Member { object, member } => {
                self.format_expr(object);
                self.push(".");
                self.push(&member.text);
            }
            ExprKind::StructLiteral { name, fields } => {
                self.format_path(name);
                // `Point {}` — every field defaulted — is written without the
                // space a field would have needed.
                if fields.is_empty() {
                    self.push(" {}");
                    return;
                }
                self.push(" { ");
                self.format_field_inits(fields);
                self.push(" }");
            }
            ExprKind::New { name, fields } => {
                // Construction uses parentheses: `new Class(field: value)`
                self.push("new ");
                self.format_path(name);
                self.push("(");
                self.format_field_inits(fields);
                self.push(")");
            }
            ExprKind::Array(elements) => {
                self.push("[");
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.format_expr(el);
                }
                self.push("]");
            }
            ExprKind::ArrayRepeat { element, count } => {
                self.push("[");
                self.format_expr(element);
                self.push("; ");
                self.format_expr(count);
                self.push("]");
            }
            ExprKind::Weak(inner) => {
                // `weak(value)` or `weak()`
                self.push("weak(");
                if let Some(expr) = inner {
                    self.format_expr(expr);
                }
                self.push(")");
            }
            ExprKind::Lambda(lambda) => {
                // Lambda syntax: `(params): Type { body }`
                // No `func` prefix.
                self.push("(");
                for (i, param) in lambda.parameters.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.push(&param.name.text);
                    if let Some(t) = &param.type_ref {
                        self.push(": ");
                        self.format_type(t);
                    }
                }
                self.push(")");
                if let Some(ret) = &lambda.return_type {
                    self.push(" -> ");
                    self.format_type(ret);
                }
                if lambda.is_expression {
                    self.push(" => ");
                    if let Some(Statement {
                        kind: StatementKind::Return(Some(expr)),
                        ..
                    }) = lambda.body.statements.first()
                    {
                        self.format_expr(expr);
                    }
                } else {
                    self.push(" ");
                    self.format_block_inline(&lambda.body);
                }
            }
            ExprKind::Try(inner) => {
                // Postfix `?`: `expr?`
                self.format_expr(inner);
                self.push("?");
            }
            ExprKind::Index { object, index } => {
                self.format_expr(object);
                self.push("[");
                self.format_expr(index);
                self.push("]");
            }
            ExprKind::Slice { object, start, end } => {
                self.format_expr(object);
                self.push("[");
                self.format_expr(start);
                self.push("..");
                self.format_expr(end);
                self.push("]");
            }
        }
    }

    fn format_field_inits(&mut self, fields: &[FieldInit]) {
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(&field.name.text);
            self.push(": ");
            self.format_expr(&field.value);
        }
    }

    // ── Paths and types ──────────────────────────────────────────────────

    fn format_path(&mut self, path: &Path) {
        if let Some(m) = &path.module {
            self.push(&m.text);
            self.push(".");
        }
        self.push(&path.name.text);
    }

    fn format_type(&mut self, t: &TypeRef) {
        match t {
            TypeRef::Named(path) => self.format_path(path),
            TypeRef::Option { element, .. } => {
                self.push("Option<");
                self.format_type(element);
                self.push(">");
            }
            TypeRef::Result { ok, err, .. } => {
                self.push("Result<");
                self.format_type(ok);
                self.push(", ");
                self.format_type(err);
                self.push(">");
            }
            TypeRef::Weak { class, .. } => {
                self.push("weak ");
                self.format_path(class);
            }
            TypeRef::Array { element, .. } => {
                self.push("[]");
                self.format_type(element);
            }
            TypeRef::FixedArray { element, size, .. } => {
                self.push("[");
                self.format_expr(size);
                self.push("]");
                self.format_type(element);
            }
            TypeRef::Function {
                parameters,
                return_type,
                ..
            } => {
                // Function type: `(int, int) -> int`
                self.push("(");
                for (i, p) in parameters.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.format_type(p);
                }
                self.push(")");
                if let Some(ret) = return_type {
                    self.push(" -> ");
                    self.format_type(ret);
                }
            }
            TypeRef::Pointer { pointee, .. } => {
                self.push("*");
                self.format_type(pointee);
            }
        }
    }
}

fn binary_op_str(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "||",
        BinaryOp::And => "&&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::BitAnd => "&",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Less => "<",
        BinaryOp::Greater => ">",
        BinaryOp::LessEqual => "<=",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Modulo => "%",
    }
}

fn assignment_op_str(op: AssignmentOp) -> &'static str {
    match op {
        AssignmentOp::Assign => "=",
        AssignmentOp::Add => "+=",
        AssignmentOp::Subtract => "-=",
        AssignmentOp::Multiply => "*=",
        AssignmentOp::Divide => "/=",
        AssignmentOp::BitAnd => "&=",
        AssignmentOp::BitOr => "|=",
        AssignmentOp::BitXor => "^=",
        AssignmentOp::ShiftLeft => "<<=",
        AssignmentOp::ShiftRight => ">>=",
    }
}

#[cfg(test)]
mod tests;
