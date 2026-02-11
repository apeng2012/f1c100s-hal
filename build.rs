use std::fmt::Write;
use std::path::PathBuf;
use std::{env, fs};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // F1C100S GPIO ports with actual pin counts:
    // PA: 4 pins (PA0-PA3)
    // PB: 4 pins (PB0-PB3)
    // PC: 4 pins (PC0-PC3)
    // PD: 22 pins (PD0-PD21)
    // PE: 13 pins (PE0-PE12)
    // PF: 6 pins (PF0-PF5)
    let gpio_ports: &[(&char, u8)] = &[(&'A', 4), (&'B', 4), (&'C', 4), (&'D', 22), (&'E', 13), (&'F', 6)];

    // Generate singletons
    let mut singletons: Vec<String> = Vec::new();

    // Add GPIO pin singletons
    for (port, count) in gpio_ports {
        for pin_num in 0..*count {
            singletons.push(format!("P{}{}", port, pin_num));
        }
    }

    // Add peripheral singletons
    singletons.push("CCU".to_string());
    singletons.push("PIO".to_string());
    singletons.push("TIMER".to_string());
    singletons.push("UART0".to_string());
    singletons.push("UART1".to_string());
    singletons.push("UART2".to_string());
    singletons.push("SPI0".to_string());
    singletons.push("SPI1".to_string());

    // _generated.rs
    let mut g = TokenStream::new();

    let singleton_tokens: Vec<_> = singletons.iter().map(|s| format_ident!("{}", s)).collect();

    g.extend(quote! {
        crate::peripherals_definition!(#(#singleton_tokens),*);
    });

    g.extend(quote! {
        crate::peripherals_struct!(#(#singleton_tokens),*);
    });

    // Generate init_gpio function (empty for now)
    g.extend(quote! {
        pub unsafe fn init_gpio() {
            // GPIO clock enable will be handled by CCU
        }
    });

    // _macros.rs
    let mut m = String::new();

    // Generate foreach_pin macro
    let mut pins_table: Vec<Vec<String>> = Vec::new();
    for (port_num, (port, count)) in gpio_ports.iter().enumerate() {
        for pin_num in 0..*count {
            let pin_name = format!("P{}{}", port, pin_num);
            pins_table.push(vec![
                pin_name,
                format!("PIO{}", port),
                port_num.to_string(),
                pin_num.to_string(),
            ]);
        }
    }

    make_table(&mut m, "foreach_pin", &pins_table);

    // Generate empty foreach_peripheral macro
    let peripherals_table: Vec<Vec<String>> = vec![];
    make_table(&mut m, "foreach_peripheral", &peripherals_table);

    // Generate empty foreach_interrupt macro
    let interrupts_table: Vec<Vec<String>> = vec![];
    make_table(&mut m, "foreach_interrupt", &interrupts_table);

    // Write generated files
    let out_file = out_dir.join("_generated.rs").to_string_lossy().to_string();
    fs::write(out_file, g.to_string()).unwrap();

    let out_file = out_dir.join("_macros.rs").to_string_lossy().to_string();
    fs::write(out_file, m).unwrap();

    // 根据 spl feature 选择内存布局，生成 memory.x 到 OUT_DIR
    // arm9-rt 的 link.x 会 INCLUDE memory.x
    let memory_x_content = if env::var("CARGO_FEATURE_SPL").is_ok() {
        include_str!("memory-spl.x")
    } else {
        include_str!("memory-default.x")
    };
    fs::write(out_dir.join("memory.x"), memory_x_content).unwrap();
    println!("cargo:rustc-link-search={}", out_dir.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=memory-default.x");
    println!("cargo:rerun-if-changed=memory-spl.x");
}

fn make_table(out: &mut String, name: &str, data: &Vec<Vec<String>>) {
    write!(
        out,
        "#[allow(unused)]
macro_rules! {} {{
    ($($pat:tt => $code:tt;)*) => {{
        macro_rules! __{}_inner {{
            $(($pat) => $code;)*
            ($_:tt) => {{}}
        }}
",
        name, name
    )
    .unwrap();

    for row in data {
        writeln!(out, "        __{}_inner!(({}));", name, row.join(",")).unwrap();
    }

    write!(
        out,
        "    }};
}}"
    )
    .unwrap();
}
