pub fn demangle_symbol(name: &str) -> String {
    let mut demangled = if let Ok(demangled) = cpp_demangle::Symbol::new(name) {
        decode_rust_type(&demangled.to_string())
    } else if let Ok(demangled) = rustc_demangle::try_demangle(name) {
        decode_rust_type(&demangled.to_string())
    } else if let Some(manual_demangled) = try_manual_demangle(name) {
        decode_rust_type(&manual_demangled)
    } else {
        decode_rust_type(name)
    };

    if let Ok(train_fn_impl) = crate::symbol::TraitFnImpl::parse(&demangled) {
        demangled = train_fn_impl.to_string(); // TODO: don't waste this parsing
    }
    // Some function names ends with e.g. ::hdfea6b6d53cc7e8c - strip that:
    if let Some(hash_pos) = demangled.rfind("::h") {
        demangled = demangled[..hash_pos].to_owned();
    }

    demangled
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
        .replace("$u3b$", ";")
        .replace("$u7b$", "{")
        .replace("$u7d$", "}")
        .replace("$LT$", "<")
        .replace("$GT$", ">")
        .replace("$C$", ",")
}

/// Try to manually demangle Itanium ABI symbols that standard demanglers can't handle
fn try_manual_demangle(name: &str) -> Option<String> {
    if !name.starts_with("__ZN") {
        return None;
    }

    let mut input = &name[4..]; // Skip "__ZN"
    let mut components = Vec::new();

    // Parse length-prefixed components
    while !input.is_empty() && input.chars().next()?.is_ascii_digit() {
        let (length, remaining) = parse_length_prefix(input)?;

        if remaining.len() < length {
            break;
        }

        let (component, rest) = remaining.split_at(length);
        components.push(component);
        input = rest;

        // Stop if we hit a non-digit (start of hash or other suffix)
        if !input.is_empty() && !input.chars().next()?.is_ascii_digit() {
            break;
        }
    }

    if components.is_empty() {
        return None;
    }

    Some(components.join("::"))
}

/// Parse a decimal length prefix from the start of a string
fn parse_length_prefix(input: &str) -> Option<(usize, &str)> {
    let mut end = 0;
    for (i, ch) in input.char_indices() {
        if ch.is_ascii_digit() {
            end = i + 1;
        } else {
            break;
        }
    }

    if end == 0 {
        return None;
    }

    let length_str = &input[..end];
    let length = length_str.parse().ok()?;
    let remaining = &input[end..];

    Some((length, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manual_demangle_egui_symbol() {
        let mangled = "__ZN4egui7context27IMMEDIATE_VIEWPORT_RENDERER29_$u7b$$u7b$constant$u7d$$u7d$28_$u7b$$u7b$closure$u7d$$u7d$3VAL17hef349e8e72b897f3E$tlv$init";
        let demangled = demangle_symbol(mangled);

        // Should extract the namespace components and decode Unicode escapes
        assert!(demangled.contains("egui::context::IMMEDIATE_VIEWPORT_RENDERER"));
        assert!(demangled.contains("{{constant}}"));
        assert!(demangled.contains("{{closure}}"));
        assert!(!demangled.contains("$u7b$"));
        assert!(!demangled.contains("$u7d$"));
    }

    #[test]
    fn test_parse_length_prefix() {
        assert_eq!(parse_length_prefix("4egui"), Some((4, "egui")));
        assert_eq!(
            parse_length_prefix("27IMMEDIATE_VIEWPORT_RENDERER"),
            Some((27, "IMMEDIATE_VIEWPORT_RENDERER"))
        );
        assert_eq!(parse_length_prefix("123abc"), Some((123, "abc")));
        assert_eq!(parse_length_prefix("abc"), None);
        assert_eq!(parse_length_prefix(""), None);
    }

    #[test]
    fn test_try_manual_demangle() {
        assert_eq!(
            try_manual_demangle("__ZN4egui7context"),
            Some("egui::context".to_owned())
        );
        assert_eq!(
            try_manual_demangle("__ZN4test5hello17hef349e8e72b897f3E"),
            Some("test::hello::hef349e8e72b897f3".to_owned())
        );
        assert_eq!(try_manual_demangle("regular_symbol"), None);
        assert_eq!(try_manual_demangle("__Z"), None);
    }

    #[test]
    fn test_decode_rust_type() {
        assert_eq!(
            decode_rust_type("$u7b$$u7b$constant$u7d$$u7d$"),
            "{{constant}}"
        );
        assert_eq!(decode_rust_type("$u3b$test$u20$space"), ";test space");
        assert_eq!(decode_rust_type("$LT$T$GT$"), "<T>");
        assert_eq!(decode_rust_type("normal_text"), "normal_text");
    }
}
