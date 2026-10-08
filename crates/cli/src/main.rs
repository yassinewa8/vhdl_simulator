//! Headless command-line tool. Only prints its version for now; Phase 1
//! adds `cli run <files...> --top <entity>`.

fn main() {
    println!("{}", version_line());
}

/// One-line description printed when the tool starts.
fn version_line() -> String {
    format!(
        "{} cli {} (no commands yet)",
        studio_core::PRODUCT_NAME,
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_product() {
        assert!(version_line().starts_with("NVC Studio cli"));
    }
}
