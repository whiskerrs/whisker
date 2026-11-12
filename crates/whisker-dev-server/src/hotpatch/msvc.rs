//! The patch link for MSVC's `link.exe`: a captured program link
//! reshaped into a DLL link of the thin objects.
//!
//! A PE image resolves nothing against the program that loads it, so
//! unlike the ELF and Mach-O plans there is no "leave it undefined"
//! directive. Every Host function the patch calls is defined by the
//! stub object; what remains comes from the system import libraries the
//! captured line names, and anything neither covers fails the link.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::link_plan::LinkPlan;

/// The symbol subsecond looks up in a loaded patch to find its base.
const ANCHOR: &str = "whisker_aslr_anchor";

/// Options that describe the captured program image or its debug
/// output; the patch link sets its own.
const DROPPED_OPTIONS: &[&str] = &[
    "out:",
    "entry:",
    "subsystem:",
    "export:",
    "opt:",
    "pdb:",
    "pdbaltpath:",
    "natvis:",
    "def:",
    "implib:",
    "wholearchive:",
    "manifest",
    "incremental",
    "debug",
    "dll",
];

/// Build the `link.exe` invocation for a patch DLL. `captured_args`
/// must already have its response files expanded.
pub fn build_link_plan(
    captured_args: &[String],
    new_objects: &[PathBuf],
    extra_objects: &[PathBuf],
    output: &Path,
    envs: BTreeMap<String, String>,
) -> LinkPlan {
    let mut args: Vec<String> = captured_args
        .iter()
        .filter(|arg| keeps(arg))
        .cloned()
        .collect();
    args.extend([
        "/DLL".to_string(),
        "/DEBUG".to_string(),
        format!("/PDB:{}", output.with_extension("pdb").display()),
        // Nothing in the patch is reachable from an entry point; without
        // NOREF the linker would drop every patched function.
        "/OPT:NOREF,NOICF".to_string(),
        "/INCREMENTAL:NO".to_string(),
        "/NOIMPLIB".to_string(),
        format!("/EXPORT:{ANCHOR}"),
    ]);
    args.extend(
        extra_objects
            .iter()
            .chain(new_objects)
            .map(|object| object.display().to_string()),
    );
    args.push(format!("/OUT:{}", output.display()));
    LinkPlan {
        args,
        output: output.to_path_buf(),
        envs,
        // 256 unit objects overflow the 32k-character command line.
        response_file: Some(output.with_extension("rsp")),
    }
}

/// Whether a captured argument carries over to the patch link.
fn keeps(arg: &str) -> bool {
    if let Some(option) = arg.strip_prefix('/').or_else(|| arg.strip_prefix('-')) {
        let option = option.to_ascii_lowercase();
        return !DROPPED_OPTIONS
            .iter()
            .any(|dropped| option.starts_with(dropped));
    }
    // Of the inputs only system import libraries stay; they are named
    // bare and found through `LIB`. Objects, rlibs and libraries given
    // by path are the Host's own code, which the stubs stand in for.
    let bare = !arg.contains(['\\', '/', ':']);
    bare && arg.to_ascii_lowercase().ends_with(".lib")
}

/// Write `args` as a `link.exe` response file: UTF-16 with a byte-order
/// mark, one quoted argument per line.
pub fn write_response_file(path: &Path, args: &[String]) -> std::io::Result<()> {
    let mut text = String::new();
    for arg in args {
        text.push('"');
        text.push_str(&arg.replace('"', "\\\""));
        text.push_str("\"\r\n");
    }
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real rustc link line for the generated Windows
    /// Host, taken from a capture on x86_64-pc-windows-msvc.
    fn captured() -> Vec<String> {
        [
            "/NOLOGO",
            r"C:\t\debug\deps\app.0abc.rcgu.o",
            r"C:\t\debug\deps\libwhisker_news-e9cc.rlib",
            r"C:\Users\u\AppData\Local\whisker\nuget\Foundation\lib\Microsoft.WindowsAppRuntime.Bootstrap.lib",
            "windowsapp.lib",
            "user32.lib",
            "kernel32.lib",
            "/defaultlib:msvcrt",
            "/NXCOMPAT",
            r"/LIBPATH:C:\Users\u\AppData\Local\whisker\nuget\Host\lib\x64",
            r"/OUT:C:\t\debug\deps\app.exe",
            "/SUBSYSTEM:windows",
            "/ENTRY:mainCRTStartup",
            "/OPT:REF,NOICF",
            "/DEBUG",
            "/PDBALTPATH:%_PDB%",
            r"/NATVIS:C:\rust\etc\libcore.natvis",
            "/EXPORT:whisker_aslr_anchor",
            "/OPT:NOREF",
            r"C:\t\debug\build\app-1\out\.whisker-windows-0.lib",
        ]
        .map(String::from)
        .to_vec()
    }

    fn plan() -> LinkPlan {
        build_link_plan(
            &captured(),
            &[PathBuf::from(r"C:\p\objects\a.rcgu.o")],
            &[PathBuf::from(r"C:\p\aslr-stub.o")],
            Path::new(r"C:\p\whisker_news.dll"),
            BTreeMap::new(),
        )
    }

    #[test]
    fn plan_keeps_search_paths_and_system_libraries() {
        let args = plan().args;
        for kept in [
            "/NOLOGO",
            "windowsapp.lib",
            "user32.lib",
            "kernel32.lib",
            "/defaultlib:msvcrt",
            "/NXCOMPAT",
            r"/LIBPATH:C:\Users\u\AppData\Local\whisker\nuget\Host\lib\x64",
        ] {
            assert!(
                args.iter().any(|arg| arg == kept),
                "missing {kept}: {args:?}"
            );
        }
    }

    #[test]
    fn plan_drops_the_programs_inputs_and_image_options() {
        let args = plan().args;
        for dropped in [
            "app.0abc.rcgu.o",
            ".rlib",
            "Bootstrap.lib",
            ".whisker-windows-0.lib",
            "app.exe",
            "/SUBSYSTEM",
            "/ENTRY",
            "/OPT:REF",
            "PDBALTPATH",
            "NATVIS",
        ] {
            assert!(
                !args.iter().any(|arg| arg.contains(dropped)),
                "{dropped} survived: {args:?}",
            );
        }
        assert_eq!(
            args.iter()
                .filter(|arg| arg.starts_with("/EXPORT:"))
                .count(),
            1
        );
        assert_eq!(
            args.iter().filter(|arg| arg.starts_with("/OPT:")).count(),
            1
        );
    }

    #[test]
    fn plan_links_a_dll_from_the_stub_and_the_thin_objects() {
        let plan = plan();
        let tail: Vec<&str> = plan.args.iter().rev().take(3).map(String::as_str).collect();
        assert_eq!(
            tail,
            [
                r"/OUT:C:\p\whisker_news.dll",
                r"C:\p\objects\a.rcgu.o",
                r"C:\p\aslr-stub.o",
            ],
        );
        for added in ["/DLL", "/OPT:NOREF,NOICF", "/EXPORT:whisker_aslr_anchor"] {
            assert!(plan.args.iter().any(|arg| arg == added), "missing {added}");
        }
        assert!(
            plan.args
                .contains(&r"/PDB:C:\p\whisker_news.pdb".to_string())
        );
        assert_eq!(
            plan.response_file,
            Some(PathBuf::from(r"C:\p\whisker_news.rsp"))
        );
    }

    #[test]
    fn response_file_is_utf16_with_quoted_lines() {
        let dir = std::env::temp_dir().join(format!("whisker-msvc-rsp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("link.rsp");
        write_response_file(&path, &["/DLL".into(), r"C:\a b\x.o".into()]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..2], [0xFF, 0xFE]);
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&units).unwrap(),
            "\"/DLL\"\r\n\"C:\\a b\\x.o\"\r\n",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
