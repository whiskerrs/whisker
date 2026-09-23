//! Shared Xcode schemes whose target references use the same stable IDs as PBX.
use super::*;
fn xml(s: &str) -> String {
    escape_xml(s)
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
        .replace('\t', "&#9;")
}
fn reference(id: &str, apple: &AppleProjectIr, container: &str) -> Result<String> {
    let target = &apple.targets[id];
    let name = if target.kind == AppleTargetKind::Native {
        let (ext, _, prefix) = product_kind(target)?;
        format!("{prefix}{}{ext}", target.product_name)
    } else {
        target.product_name.clone()
    };
    Ok(format!(
        "<BuildableReference BuildableIdentifier=\"primary\" BlueprintIdentifier=\"{}\" BuildableName=\"{}\" BlueprintName=\"{}\" ReferencedContainer=\"container:{}\"/>",
        pbxproj_uuid(&format!("target:{id}")),
        xml(&name),
        xml(&target.product_name),
        xml(container)
    ))
}
fn actions(
    scripts: &[AppleSchemeScript],
    name: &str,
    apple: &AppleProjectIr,
    container: &str,
) -> Result<String> {
    if scripts.is_empty() {
        return Ok(String::new());
    }
    let mut out = format!("<{name}>");
    for s in scripts {
        out += &format!(
            "<ExecutionAction ActionType=\"Xcode.IDEStandardExecutionActionsCore.ExecutionActionType.ShellScriptAction\"><ActionContent title=\"{}\" scriptText=\"{}\">",
            xml(&s.name),
            xml(&s.script)
        );
        if let Some(target) = &s.environment_target {
            out += &format!(
                "<EnvironmentBuildable>{}</EnvironmentBuildable>",
                reference(target, apple, container)?
            );
        }
        out += "</ActionContent></ExecutionAction>";
    }
    Ok(out + &format!("</{name}>"))
}
fn options(
    s: &AppleScheme,
    action: AppleSchemeAction,
    apple: &AppleProjectIr,
    container: &str,
) -> Result<String> {
    let Some(o) = s.action_options.get(&action) else {
        return Ok(String::new());
    };
    ensure!(
        action != AppleSchemeAction::Analyze,
        "Analyze action options are unsupported by the Xcode renderer"
    );
    ensure!(
        matches!(
            action,
            AppleSchemeAction::Run | AppleSchemeAction::Test | AppleSchemeAction::Profile
        ) || (o.arguments.is_empty() && o.environment.is_empty()),
        "scheme arguments/environment unsupported for {action:?}"
    );
    let mut out = actions(&o.pre_actions, "PreActions", apple, container)?
        + &actions(&o.post_actions, "PostActions", apple, container)?;
    if !o.arguments.is_empty() {
        out += "<CommandLineArguments>";
        for arg in &o.arguments {
            out += &format!(
                "<CommandLineArgument argument=\"{}\" isEnabled=\"YES\"/>",
                xml(arg)
            );
        }
        out += "</CommandLineArguments>";
    }
    if !o.environment.is_empty() {
        out += "<EnvironmentVariables>";
        for (k, v) in &o.environment {
            out += &format!(
                "<EnvironmentVariable key=\"{}\" value=\"{}\" isEnabled=\"YES\"/>",
                xml(k),
                xml(v)
            );
        }
        out += "</EnvironmentVariables>";
    }
    Ok(out)
}
pub(super) fn render(s: &AppleScheme, apple: &AppleProjectIr, container: &str) -> Result<String> {
    let yes = |v: Option<bool>| if v.unwrap_or(true) { "YES" } else { "NO" };
    let mut out = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Scheme LastUpgradeVersion=\"1600\" version=\"1.7\"><BuildAction parallelizeBuildables=\"YES\" buildImplicitDependencies=\"YES\">".to_string();
    out += &options(s, AppleSchemeAction::Build, apple, container)?;
    out += "<BuildActionEntries>";
    for target in &s.build_targets {
        let f = s.build_for.get(target).cloned().unwrap_or_default();
        out += &format!(
            "<BuildActionEntry buildForTesting=\"{}\" buildForRunning=\"{}\" buildForProfiling=\"{}\" buildForArchiving=\"{}\" buildForAnalyzing=\"{}\">{}</BuildActionEntry>",
            yes(f.testing),
            yes(f.running),
            yes(f.profiling),
            yes(f.archiving),
            yes(f.analyzing),
            reference(target, apple, container)?
        );
    }
    out += "</BuildActionEntries></BuildAction>";
    out += &format!(
        "<TestAction buildConfiguration=\"{}\" shouldUseLaunchSchemeArgsEnv=\"{}\">",
        xml(s
            .test_configuration
            .as_deref()
            .unwrap_or(&s.run_configuration)),
        if s.action_options.contains_key(&AppleSchemeAction::Test) {
            "NO"
        } else {
            "YES"
        }
    );
    out += &options(s, AppleSchemeAction::Test, apple, container)?;
    out += "<Testables>";
    for id in &s.test_targets {
        out += &format!(
            "<TestableReference skipped=\"NO\">{}</TestableReference>",
            reference(id, apple, container)?
        );
    }
    out += "</Testables>";
    if !s.test_plans.is_empty() {
        out += "<TestPlans>";
        for p in &s.test_plans {
            out += &format!(
                "<TestPlanReference reference=\"container:{}\" default=\"{}\"/>",
                xml(p.as_str()),
                if s.default_test_plan.as_ref() == Some(p) {
                    "YES"
                } else {
                    "NO"
                }
            );
        }
        out += "</TestPlans>";
    }
    out += "</TestAction>";
    for (action, tag, configuration) in [
        (
            AppleSchemeAction::Run,
            "LaunchAction",
            s.run_configuration.as_str(),
        ),
        (
            AppleSchemeAction::Profile,
            "ProfileAction",
            s.profile_configuration
                .as_deref()
                .unwrap_or(&s.archive_configuration),
        ),
    ] {
        out += &format!(
            "<{tag} buildConfiguration=\"{}\" selectedDebuggerIdentifier=\"Xcode.DebuggerFoundation.Debugger.LLDB\" selectedLauncherIdentifier=\"Xcode.IDEFoundation.Launcher.LLDB\" useCustomWorkingDirectory=\"NO\">",
            xml(configuration)
        );
        out += &options(s, action, apple, container)?;
        if let Some(id) = &s.run_target {
            out += &format!(
                "<BuildableProductRunnable runnableDebuggingMode=\"0\">{}</BuildableProductRunnable>",
                reference(id, apple, container)?
            );
        }
        out += &format!("</{tag}>");
    }
    options(s, AppleSchemeAction::Analyze, apple, container)?;
    out += &format!(
        "<AnalyzeAction buildConfiguration=\"{}\"/><ArchiveAction buildConfiguration=\"{}\" revealArchiveInOrganizer=\"YES\">{}</ArchiveAction></Scheme>\n",
        xml(s
            .analyze_configuration
            .as_deref()
            .unwrap_or(&s.run_configuration)),
        xml(&s.archive_configuration),
        options(s, AppleSchemeAction::Archive, apple, container)?
    );
    Ok(out)
}
