use dsb::cell::Style;

macro_rules! theme {
    ($($x:literal $color:literal $($style:expr)?),+ $(,)?) => {
        #[rustfmt::skip]
        pub const NAMES: [&str; [$($x),+].len()] = [$($x),+];
        #[rustfmt::skip]
        pub const COLORS: [[u8; 3]; NAMES.len()] = car::map!([$($color),+], |x| crate::text::color(x));
        pub const STYLES: [u8; NAMES.len()] = [$(
            ($($style, )? 0, ).0
        ),+];
    };
}

macro_rules! modified {
        ($count:literal $($x:literal . $mod:literal $color:literal $($style:expr)?,)+ $(,)?) => {
            pub const MODIFIED: [(&str, &str); $count] = [
                $(($x, $mod),)+
            ];
            pub const MCOLORS: [[u8;3]; MODIFIED.len()] = car::map!([$($color),+], |x| crate::text::color(x));
            pub const MSTYLE: [u8; MODIFIED.len()] = [$(($($style, )? 0, ).0 ,)+];
        }
}
pub(crate) use theme;
theme! {
"constructor" b"#FFAD66",
"field" b"#cccac2",

"comment" b"#5c6773" Style::ITALIC,
// "decorator" b"#cccac2",
"function" b"#FFD173" Style::ITALIC,
"interface" b"#5CCFE6",
"keyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
"macro" b"#fbc351" Style::BOLD,
"method" b"#FFD173" Style::ITALIC,
// "namespace" b"#cccac2",
"number" b"#dfbfff",
"operator" b"#F29E74",
// "property" b"#cccac2",
"string" b"#D5FF80",
// "struct" b"#cccac2",
// "typeParameter" b"#cccac2",
"class" b"#73b9ff",
"enumMember" b"#73b9ff",
"enum" b"#73b9ff" Style::ITALIC | Style::BOLD,
"builtinType" b"#73d0ff" Style::ITALIC,
// "type" b"#73d0ff" Style::ITALIC | Style::BOLD,
"typeAlias" b"#69caed" Style::ITALIC | Style::BOLD,
"struct" b"#73d0ff" Style::ITALIC | Style::BOLD,
"variable" b"#cccac2",
// "angle" b"#cccac2",
// "arithmetic" b"#cccac2",
// "attributeBracket" b"#cccac2",
"parameter" b"#DFBFFF",
"namespace" b"#73d0ff",
// "attributeBracket" b"#cccac2",
// "attribute" b"#cccac2",
// "bitwise" b"#cccac2",
// "boolean" b"#cccac2",
// "brace" b"#cccac2",
// "bracket" b"#cccac2",
// "builtinAttribute" b"#cccac2",
// "character" b"#cccac2",
// "colon" b"#cccac2",
// "comma" b"#cccac2",
// "comparison" b"#cccac2",
// "constParameter" b"#cccac2",
"const" b"#DFBFFF",
// "deriveHelper" b"#cccac2",
// "derive" b"#cccac2",
// "dot" b"#cccac2",
// "escapeSequence" b"#cccac2",
// "formatSpecifier" b"#cccac2",
// "generic" b"#cccac2",
// "invalidEscapeSequence" b"#cccac2",
// "label" b"#cccac2",
// "lifetime" b"#cccac2",
// "logical" b"#cccac2",
"macroBang" b"#f28f74",
// "parenthesis" b"#cccac2",
// "procMacro" b"#cccac2",
// "punctuation" b"#cccac2",
"selfKeyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
"selfTypeKeyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
// "semicolon" b"#cccac2",
// "static" b"#cccac2",
// "toolModule" b"#cccac2",
// "union" b"#cccac2",
// "unresolvedReference" b"#cccac2",
}

modified! { 2
    "function" . "unsafe" b"#F28779",
    "variable" . "mutable" b"#e6dab6",
}
