use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("token-audit {}", token_audit_core::version());
            ExitCode::SUCCESS
        }
        None | Some("--help" | "-h") => {
            println!("用法：token-audit --version");
            ExitCode::SUCCESS
        }
        Some(_) => {
            eprintln!("未知参数。运行 token-audit --help 查看用法。");
            ExitCode::FAILURE
        }
    }
}
