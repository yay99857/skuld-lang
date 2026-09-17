" Vim syntax file for Skuld.
"
" Every keyword, escape and literal form here is taken from the lexer in
" `compiler/src/lexer.rs`, not from the roadmap: this file describes what the
" compiler accepts today. When a milestone adds a keyword, add it here too.
"
" `impl` is lexed and has no meaning yet, so it is listed as a keyword and
" nothing more. `union`, `packed` and `align` are ordinary identifiers except
" in one position each, which a regex highlighter cannot tell apart reliably;
" they are left alone rather than coloured everywhere.

if exists('b:current_syntax')
  finish
endif

syn case match

" --- Comments -------------------------------------------------------------
" The lexer knows `//` only; there is no block comment form.
syn keyword skuldTodo contained TODO FIXME XXX NOTE
syn match skuldComment +//.*$+ contains=skuldTodo,@Spell

" --- Keywords -------------------------------------------------------------
syn keyword skuldKeyword     func let var new pub static const
syn keyword skuldKeyword     class struct enum impl interface
syn keyword skuldConditional if else match
syn keyword skuldRepeat      while loop for in
syn keyword skuldStatement   return break continue defer
syn keyword skuldInclude     import
syn keyword skuldUnsafe      unsafe extern
syn keyword skuldStorage     weak
syn keyword skuldSelf        this

" --- Types ----------------------------------------------------------------
" `int` is the spelling of i64 and `float` of f64; both widths are listed
" because the resolver registers each spelling as its own conversion.
syn keyword skuldType int float bool string void char
syn keyword skuldType i8 i16 i32 i64 isize
syn keyword skuldType u8 u16 u32 u64 usize
syn keyword skuldType Option Result

" A user type is a capitalised identifier, which is convention rather than a
" rule: the compiler enforces no case. `pub` is what exports, not spelling.
syn match skuldUserType /\<\u\w*\>/

" --- Prelude --------------------------------------------------------------
" The bindings `compiler/src/resolver.rs` inserts into the root scope. All of
" them are shadowable, so this is a hint, not a guarantee.
syn keyword skuldBuiltin print ptr bytes_to_string
" The pointer builtins, which the checker refuses outside an `unsafe` block,
" and the two that ask about a declared layout.
syn keyword skuldUnsafeBuiltin load store volatile_load volatile_store
syn keyword skuldUnsafeBuiltin offset addr ptr_from
syn keyword skuldBuiltin size_of offset_of
syn keyword skuldConstant Some None null Ok Err
syn keyword skuldBoolean  true false

" --- Functions ------------------------------------------------------------
syn match skuldFunction /\<func\s\+\zs\w\+/
syn match skuldFunction /\<\w\+\ze\s*(/

" --- Builtin methods ------------------------------------------------------
" Checked by the type checker on arrays, strings, Result and weak references.
syn match skuldMethod /\.\@<=\<\(len\|push\|insert\|pop\|remove\|sort\|to_sorted\)\>/
syn match skuldMethod /\.\@<=\<\(bytes\|is_ok\|is_err\|upgrade\|alive\|get\)\>/

" --- Numbers --------------------------------------------------------------
" Hexadecimal, binary and octal prefixes, with `_` as a separator in every
" base. A float is decimal only and needs a digit after the dot, which is
" exactly why `0..5` lexes as a range and not as a malformed float.
syn match skuldNumber /\<0x[0-9A-Fa-f_]\+\>/
syn match skuldNumber /\<0b[01_]\+\>/
syn match skuldNumber /\<0o[0-7_]\+\>/
syn match skuldNumber /\<\d[0-9_]*\>/
syn match skuldFloat  /\<\d[0-9_]*\.\d[0-9_]*\>/

" --- Strings and chars ----------------------------------------------------
" The complete escape set. Anything else is `E0004` at compile time, so it is
" flagged here rather than passed off as valid.
syn match skuldEscapeError   contained /\\./
syn match skuldEscape        contained /\\[\\"'nrt0$]/
syn match skuldInterpDelim   contained /\${/
syn match skuldInterpDelim   contained /}/

syn cluster skuldInterpolated contains=skuldKeyword,skuldConditional,skuldRepeat,
      \skuldStatement,skuldType,skuldUserType,skuldBuiltin,skuldConstant,
      \skuldBoolean,skuldMethod,skuldFunction,skuldNumber,skuldFloat,
      \skuldOperator,skuldString,skuldChar

" `${ ... }` holds an ordinary expression. One level of nested braces is
" tracked so a struct literal inside an interpolation does not end it early;
" the lexer counts depth properly, a regex highlighter approximates it.
syn region skuldInterpBrace contained matchgroup=skuldInterpDelim
      \ start="{" end="}" contains=@skuldInterpolated
syn region skuldInterp contained matchgroup=skuldInterpDelim
      \ start="\${" end="}"
      \ contains=@skuldInterpolated,skuldInterpBrace

syn region skuldString start=+"+ skip=+\\\\\|\\"+ end=+"+
      \ contains=skuldEscape,skuldEscapeError,skuldInterp,@Spell
syn region skuldChar   start=+'+ skip=+\\\\\|\\'+ end=+'+
      \ contains=skuldEscape,skuldEscapeError

" --- Operators ------------------------------------------------------------
" `?` is postfix propagation on a Result; `->` and `:` both spell a return
" type; `..` is a half-open range.
syn match skuldOperator /->\|=>\|?\|\.\.=\?\|&&\|||\|!/
syn match skuldOperator /<<=\|>>=\|&=\|\|=\|\^=\|<<\|>>\|&\|\^\|\~/
syn match skuldOperator /==\|!=\|<=\|>=\|<\|>/
syn match skuldOperator /+=\|-=\|\*=\|\/=\|=/
syn match skuldOperator /+\|-\|\*\|%/
" A `/` only when it is not the `//` that opens a comment.
syn match skuldOperator /\/\(\/\)\@!/

" --- Links ----------------------------------------------------------------
hi def link skuldComment     Comment
hi def link skuldTodo        Todo
hi def link skuldKeyword     Keyword
hi def link skuldConditional Conditional
hi def link skuldRepeat      Repeat
hi def link skuldStatement   Statement
hi def link skuldInclude     Include
hi def link skuldUnsafe      Exception
hi def link skuldStorage     StorageClass
hi def link skuldType        Type
hi def link skuldUserType    Type
hi def link skuldBuiltin     Function
hi def link skuldUnsafeBuiltin Exception
hi def link skuldSelf        Identifier
hi def link skuldConstant    Constant
hi def link skuldBoolean     Boolean
hi def link skuldMethod      Function
hi def link skuldFunction    Function
hi def link skuldNumber      Number
hi def link skuldFloat       Float
hi def link skuldString      String
hi def link skuldChar        Character
hi def link skuldEscape      SpecialChar
hi def link skuldEscapeError Error
hi def link skuldInterpDelim Delimiter
hi def link skuldOperator    Operator

let b:current_syntax = 'skuld'
