use dsb::cell::Style;

super::semantic_tokens::theme! {
    "attribute" b"#ffd173",
    "comment" b"#5c6773" Style::ITALIC,
    "constant" b"#DFBFFF",
    "function" b"#FFD173" Style::ITALIC,
    "function.macro" b"#fbc351",
    "variable.builtin" b"#FFAD66",
    "keyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
    "number" b"#dfbfff",
    "operator" b"#F29E74",
    "punctuation" b"#cccac2",
    "string" b"#D5FF80",
    "tag" b"#5CCFE6"  Style::ITALIC | Style::BOLD,
    "type" b"#73D0FF"  Style::ITALIC | Style::BOLD,
    "variable" b"#cccac2",
    "variable.parameter" b"#DFBFFF",
    "namespace" b"#73d0ff",
}
