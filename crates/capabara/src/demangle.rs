pub fn demangle_symbol(name: &str) -> String {
    if let Ok(demangled) = cpp_demangle::Symbol::new(name) {
        decode_rust_type(&demangled.to_string())
    } else if let Ok(demangled) = rustc_demangle::try_demangle(name) {
        decode_rust_type(&demangled.to_string())
    } else {
        decode_rust_type(name)
    }
}

fn decode_rust_type(encoded: &str) -> String {
    encoded
        .replace("$BP$", "*")
        .replace("$RF$", "&")
        .replace("$LP$", "(")
        .replace("$RP$", ")")
        .replace("$u5b$", "[")
        .replace("$u5d$", "]")
        .replace("$u20$", " ")
        .replace("$LT$", "<")
        .replace("$GT$", ">")
        .replace("$C$", ",")
}
