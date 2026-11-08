use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Expr, FnArg, ImplItemFn, ItemFn, ItemImpl, ItemTrait, Stmt,
    visit::{self, Visit},
};

const SOURCE_ROOTS: [&str; 3] = ["src", "tests", "examples"];

struct FixedValueVisitor {
    violations: Vec<String>,
    ok_or_else_helpers: Vec<String>,
    is_inspecting_impl: bool,
}

impl<'ast> Visit<'ast> for FixedValueVisitor {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.inspect(&item.sig, &item.block);
        visit::visit_item_fn(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        let is_inspecting_impl = self.is_inspecting_impl;
        self.is_inspecting_impl = item.trait_.is_none() && item.generics.params.is_empty();
        visit::visit_item_impl(self, item);
        self.is_inspecting_impl = is_inspecting_impl;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        if self.is_inspecting_impl {
            self.inspect(&item.sig, &item.block);
        }
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_item_trait(&mut self, _item: &'ast ItemTrait) {}

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "ok_or_else"
            && let Some(Expr::Path(path)) = call.args.first()
            && let Some(segment) = path.path.segments.last()
        {
            self.ok_or_else_helpers.push(segment.ident.to_string());
        }
        visit::visit_expr_method_call(self, call);
    }
}

impl FixedValueVisitor {
    fn inspect(&mut self, signature: &syn::Signature, body: &syn::Block) {
        if matches!(signature.ident.to_string().as_str(), "new" | "default")
            || !signature.generics.params.is_empty()
            || signature
                .inputs
                .iter()
                .any(|input| !matches!(input, FnArg::Receiver(_)))
        {
            return;
        }
        let [Stmt::Expr(expression, terminator)] = body.stmts.as_slice() else {
            return;
        };
        if terminator.is_some() && !matches!(expression, Expr::Return(_)) {
            return;
        }
        if is_fixed_construction(expression) {
            self.violations.push(signature.ident.to_string());
        }
    }
}

fn get_violations(source: &str) -> Result<Vec<String>, syn::Error> {
    let syntax = syn::parse_file(source)?;
    let mut visitor = FixedValueVisitor {
        violations: Vec::new(),
        ok_or_else_helpers: Vec::new(),
        is_inspecting_impl: true,
    };
    visitor.visit_file(&syntax);
    let helpers = visitor.ok_or_else_helpers;
    let mut violations = visitor.violations;
    violations.retain(|name| !helpers.contains(name));
    Ok(violations)
}

fn is_fixed_construction(expression: &Expr) -> bool {
    match expression {
        Expr::Paren(value) => is_fixed_construction(&value.expr),
        Expr::Group(value) => is_fixed_construction(&value.expr),
        Expr::Return(value) => value.expr.as_deref().is_some_and(is_fixed_construction),
        Expr::Block(value) => match value.block.stmts.as_slice() {
            [Stmt::Expr(inner, None) | Stmt::Expr(inner @ Expr::Return(_), Some(_))] => {
                is_fixed_construction(inner)
            }
            _ => false,
        },
        Expr::Struct(value) => {
            value
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "CommandResponse")
                && value.rest.is_none()
                && value
                    .fields
                    .iter()
                    .all(|field| is_fixed_argument(&field.expr))
        }
        Expr::Call(call) => is_fixed_constructor(call),
        _ => false,
    }
}

fn is_fixed_constructor(call: &syn::ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let Some(last) = segments.last() else {
        return false;
    };
    if matches!(last.as_str(), "Ok" | "Err") {
        return call.args.len() == 1 && call.args.first().is_some_and(is_fixed_construction);
    }
    let named_error = segments.len() >= 2
        && segments[segments.len() - 2].ends_with("Error")
        && matches!(last.as_str(), "new" | "new_with_cause");
    let io_error = segments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .ends_with(&["std", "io", "Error", "other"]);
    (named_error || io_error) && call.args.iter().all(is_fixed_argument)
}

fn is_fixed_argument(expression: &Expr) -> bool {
    match expression {
        Expr::Lit(_) => true,
        Expr::Path(value) => value.path.segments.last().is_some_and(|segment| {
            let name = segment.ident.to_string();
            name.starts_with(|character: char| character.is_ascii_uppercase())
                && (value.path.segments.len() > 1
                    || name.chars().all(|character| {
                        character.is_ascii_uppercase()
                            || character.is_ascii_digit()
                            || character == '_'
                    }))
        }),
        Expr::Reference(value) => is_fixed_argument(&value.expr),
        Expr::Paren(value) => is_fixed_argument(&value.expr),
        Expr::Group(value) => is_fixed_argument(&value.expr),
        Expr::Unary(value) => {
            matches!(value.op, syn::UnOp::Neg(_) | syn::UnOp::Not(_))
                && is_fixed_argument(&value.expr)
        }
        Expr::Tuple(value) => value.elems.iter().all(is_fixed_argument),
        Expr::Array(value) => value.elems.iter().all(is_fixed_argument),
        Expr::Call(call) => is_fixed_argument_call(call),
        _ => false,
    }
}

fn is_fixed_argument_call(call: &syn::ExprCall) -> bool {
    let Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let tail = segments.iter().map(String::as_str).collect::<Vec<_>>();
    let empty_collection = (tail.ends_with(&["Vec", "new"]) || tail.ends_with(&["String", "new"]))
        && call.args.is_empty();
    let io_error = tail.ends_with(&["std", "io", "Error", "other"])
        && call.args.len() == 1
        && call.args.iter().all(is_fixed_argument);
    empty_collection || io_error
}

fn check_root(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for directory in SOURCE_ROOTS {
        let path = root.join(directory);
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "{}: source root must be a directory",
                path.display()
            ));
        }
        files.extend(get_rust_files(&path)?);
    }
    files.sort();
    let mut findings = Vec::new();
    for path in files {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let violations =
            get_violations(&source).map_err(|error| format!("{}: {error}", path.display()))?;
        findings.extend(
            violations
                .into_iter()
                .map(|name| format!("{}: {name}", path.display())),
        );
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(format!("fixed-value helpers:\n{}", findings.join("\n")))
    }
}

fn get_rust_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries =
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut files = Vec::new();
    for entry in entries {
        let kind = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", entry.path().display()))?;
        if kind.is_dir() {
            files.extend(get_rust_files(&entry.path())?);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rs")
        {
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn main() {
    if let Err(error) = check_root(Path::new(".")) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{check_root, get_violations};
    use std::fs;

    #[test]
    fn detects_fixed_errors_responses_and_return_wrappers() {
        // Given: three fixed values including constants, a nested cause and a response.
        let source = r#"
            fn fixed_error() -> SnapshotError { SnapshotError::new(Kind::Bad, MESSAGE) }
            fn fixed_wrapped() -> Result<(), Error> { return Err(CommandError::new_with_cause(Kind::Bad, MESSAGE, std::io::Error::other("cause"))); }
            fn fixed_response() -> CommandResponse { CommandResponse { result: ResponseResult::Drained, warnings: Vec::new() } }
        "#;
        // When: the syntax checker analyzes the functions.
        let violations = get_violations(source).unwrap();
        // Then: each independently named fixed constructor is reported.
        assert_eq!(
            violations,
            ["fixed_error", "fixed_wrapped", "fixed_response"]
        );
    }

    #[test]
    fn permits_trait_methods_constructors_inputs_and_dynamic_builders() {
        // Given: trait contracts, ordinary constructors, mappings and dynamic values.
        let source = r#"
            trait Service { fn default_method(&self) -> Error { Error::new(Kind::Bad) } }
            struct ServiceImpl;
            impl Service for ServiceImpl { fn default_method(&self) -> Error { Error::new(Kind::Bad) } }
            impl ServiceImpl { fn new() -> Error { Error::new(Kind::Bad) } }
            fn map_error(cause: io::Error) -> Error { Error::new_with_cause(Kind::Bad, cause) }
            fn build_response() -> CommandResponse { CommandResponse { result: obtain_result(), warnings: Vec::new() } }
            fn unsupported_nested() -> Error { Error::new(Kind::Bad, arbitrary()) }
        "#;
        // When: the checker reads their syntax.
        let violations = get_violations(source).unwrap();
        // Then: none of these meaningful or exempt functions is flagged.
        assert!(violations.is_empty());
    }

    #[test]
    fn permits_error_builders_passed_to_ok_or_else() {
        // Given: builders used from a free function and from a trait implementation.
        let source = r#"
            fn build_missing_error() -> Error { Error::new(Kind::Missing) }
            fn build_drain_error() -> Error { Error::new(Kind::Missing) }
            fn build_unused_error() -> Error { Error::new(Kind::Bad) }
            fn caller(value: Option<u8>) -> Result<u8, Error> { value.ok_or_else(build_missing_error) }
            trait Drainer { fn drain(&self) -> Result<(), Error>; }
            struct Drain;
            impl Drainer for Drain {
                fn drain(&self) -> Result<(), Error> { None.ok_or_else(build_drain_error)?; Ok(()) }
            }
        "#;
        // When: the checker inspects the source.
        let violations = get_violations(source).unwrap();
        // Then: only the builder that is not passed to ok_or_else is reported.
        assert_eq!(violations, ["build_unused_error"]);
    }

    #[test]
    fn detects_inherent_methods_and_nested_modules_without_reading_strings() {
        // Given: an inline module with a fixed inherent method and a misleading raw string.
        let source = r###"
            const TEXT: &str = r#"fn pretend() -> Error { Error::new(Kind::Bad) }"#;
            mod nested { struct S; impl S { fn fixed(&self) -> Error { Error::new(Kind::Bad) } } }
        "###;
        // When: the checker visits nested parsed items.
        let violations = get_violations(source).unwrap();
        // Then: the method is reported once and raw-string contents are ignored.
        assert_eq!(violations, ["fixed"]);
    }

    #[test]
    fn permits_dropped_values_and_unknown_error_constructors() {
        // Given: a dropped construction and a custom method not on the purity allowlist.
        let source = r#"
            fn dropped() { ExampleError::new(Kind::Bad); }
            fn dynamic() -> ExampleError { ExampleError::other("message") }
            fn dereferenced() -> ExampleError { ExampleError::new(*MUTABLE_STATIC) }
        "#;
        // When: the checker inspects the source.
        let violations = get_violations(source).unwrap();
        // Then: neither function returns an allowlisted fixed-value expression.
        assert!(violations.is_empty());
    }

    #[test]
    fn permits_generic_values_and_nonconstant_single_paths() {
        // Given: generic associated values and a PascalCase local path.
        let source = r#"
            fn generic<T: Message>() -> ExampleError { ExampleError::new(T::MESSAGE) }
            fn generic_const<const MESSAGE: u8>() -> ExampleError { ExampleError::new(MESSAGE) }
            struct S<T>(T);
            impl<T: Message> S<T> {
                fn generic_impl() -> ExampleError { ExampleError::new(T::MESSAGE) }
            }
            fn pascal() -> ExampleError { ExampleError::new(DynamicValue) }
        "#;
        // When: the checker inspects their syntax.
        let violations = get_violations(source).unwrap();
        // Then: none of the values is assumed to be a fixed constant.
        assert!(violations.is_empty());
    }

    #[test]
    fn checks_files_in_order_and_rejects_unparseable_source() {
        // Given: all required roots and two source files with fixed constructors.
        let root = tempfile::tempdir().unwrap();
        for name in ["src", "tests", "examples"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(
            root.path().join("src/z.rs"),
            "fn z() -> E { EError::new(Kind::Bad) }",
        )
        .unwrap();
        fs::write(
            root.path().join("src/a.rs"),
            "fn a() -> E { EError::new(Kind::Bad) }",
        )
        .unwrap();
        // When: the complete tree is inspected.
        let error = check_root(root.path()).unwrap_err();
        // Then: diagnostics list both functions in path order.
        assert!(error.find("a.rs: a").unwrap() < error.find("z.rs: z").unwrap());
        // When: a scanned file is replaced with malformed Rust.
        fs::write(root.path().join("src/a.rs"), "fn broken(").unwrap();
        // Then: parsing fails and names the offending file.
        assert!(check_root(root.path()).unwrap_err().contains("a.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn ignores_nested_symlinks_and_rejects_symlinked_roots() {
        use std::os::unix::fs::symlink;
        // Given: a valid source root with a symlink to invalid external Rust.
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for name in ["src", "tests", "examples"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(outside.path().join("bad.rs"), "fn broken(").unwrap();
        symlink(outside.path(), root.path().join("src/external")).unwrap();
        // When / Then: nested symlinks are ignored rather than traversed.
        assert!(check_root(root.path()).is_ok());
        // When: a required source root itself is replaced by a symlink.
        fs::remove_dir(root.path().join("examples")).unwrap();
        fs::remove_file(outside.path().join("bad.rs")).unwrap();
        symlink(outside.path(), root.path().join("examples")).unwrap();
        // Then: an incomplete scan cannot pass silently.
        assert!(check_root(root.path()).is_err());
    }

    #[test]
    fn rejects_invalid_rust_and_missing_required_roots() {
        // Given: malformed Rust and a source root missing required directories.
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/broken.rs"), "fn broken(").unwrap();
        // When / Then: malformed source and missing scan roots cannot pass silently.
        assert!(get_violations("fn broken(").is_err());
        assert!(check_root(root.path()).is_err());
    }

    #[test]
    fn rejects_unreadable_source_contents() {
        // Given: a Rust file whose bytes are not valid UTF-8.
        let root = tempfile::tempdir().unwrap();
        for name in ["src", "tests", "examples"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("src/invalid.rs"), [0xff]).unwrap();
        // When: the source tree is checked.
        let error = check_root(root.path()).unwrap_err();
        // Then: the diagnostic identifies the source that cannot be read as text.
        assert!(error.contains("invalid.rs"));
    }
}
