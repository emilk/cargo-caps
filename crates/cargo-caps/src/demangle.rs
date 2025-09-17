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

    // Some function names ends with e.g. ::hdfea6b6d53cc7e8c - strip that:
    if let Some(hash_pos) = demangled.rfind("::h") {
        demangled = demangled[..hash_pos].to_owned();
    }

    demangled = demangled.trim_start_matches('_').to_owned(); // So many things start with random count of underscores

    demangled
}

fn decode_rust_type(mut encoded: &str) -> String {
    // Find things like `$LT,GT$"` and decode into `<>`:
    let mut decoded = String::new();

    while let Some(start_dollar) = encoded.find('$') {
        decoded.push_str(&encoded[..start_dollar]);
        encoded = &encoded[start_dollar + 1..];
        if let Some(end_dollar) = encoded.find('$') {
            let contents = &encoded[..end_dollar];
            for part in contents.split(',') {
                if let Some(nr) = part.strip_prefix('u') {
                    // unicode:
                    if let Ok(nr) = u32::from_str_radix(nr, 16)
                        && let Some(c) = char::from_u32(nr)
                    {
                        decoded.push(c);
                    } else {
                        decoded.push_str(part); // fail
                    }
                } else {
                    let replacement = match part {
                        "BP" => "*",
                        "C" => ",",
                        "RF" => "&",
                        "LT" => "<",
                        "GT" => ">",
                        "LP" => "(",
                        "RP" => ")",
                        part => part,
                    };
                    decoded.push_str(replacement);
                }
            }
            encoded = &encoded[end_dollar + 1..];
        } else {
            break;
        }
    }
    decoded.push_str(encoded);

    // Second round:
    decoded.replace(" .> ", " -> ").replace("..", "::")
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
        assert!(
            demangled.contains("{{constant}}"),
            "demangled: {demangled:?}"
        );
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

    #[test]
    fn test_demangle() {
        assert_eq!(
            demangle_symbol(
                "__ZN135_$LT$extern$u20$$u22$C$u22$$u20$fn$LP$$RF$T$C$objc..runtime..Sel$RP$$u20$.$GT$$u20$R$u20$as$u20$objc..declare..MethodImplementation$GT$3imp17h8f6f1e820e818e51E"
            ),
            r#"<extern "C" fn(&T,objc::runtime::Sel) -> R as objc::declare::MethodImplementation>::imp"#
        );
    }
}
