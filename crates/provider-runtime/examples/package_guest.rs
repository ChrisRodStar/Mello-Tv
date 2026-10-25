//! Reproducibly wrap the no-WASI Rust core module as a portable component.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let input = arguments.next().ok_or("expected input core Wasm path")?;
    let output = arguments.next().ok_or("expected output component path")?;
    if arguments.next().is_some() {
        return Err("expected exactly two paths".into());
    }
    let module = std::fs::read(input)?;
    let component = wit_component::ComponentEncoder::default()
        .module(&module)?
        .validate(true)
        .encode()?;
    std::fs::write(output, component)?;
    Ok(())
}
