// Manages the bundled OpenCode sidecar so it never interferes with any OpenCode
// the user already has: it runs the *bundled* binary, on a *dedicated free port*,
// with an *app-private* XDG config/data dir, and is killed on app exit.
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;

use crate::env::Env;
use crate::opencode_config::merge_config;

#[derive(Default)]
struct RuntimeLifecycle {
    child: Option<Child>,
    url: Option<String>,
    port: Option<u16>,
    /// Epoch ms the current sidecar was spawned. The frontend needs it to tell
    /// a turn that is streaming now from one that was streaming when an earlier
    /// sidecar died: both persist identically, and only "was this message
    /// created before the process that would be producing it" separates them.
    started_at: Option<u64>,
    /// Bumped on every spawn. The exit watcher captures the value its own
    /// sidecar was spawned with and only clears the lifecycle while it still
    /// matches, so a late exit from an already-replaced sidecar cannot wipe
    /// the live one.
    generation: u64,
}

/// One lock owns every sidecar lifecycle field. Keeping child/url/port in
/// separate mutexes allowed two concurrent `start_runtime` calls to both see
/// "stopped", spawn on the same port, and overwrite each other's child handle.
#[derive(Default)]
pub struct RuntimeState {
    lifecycle: Mutex<RuntimeLifecycle>,
}

/// App-private runtime root, e.g. ~/Library/Application Support/com.happyscience.desktop/runtime
pub fn runtime_root(env: &Env) -> Result<PathBuf, String> {
    Ok(env.data_dir().join("runtime"))
}

/// The running sidecar's base URL (`http://127.0.0.1:<port>`), or None when the
/// runtime is not started yet. The gateway proxies agent calls here, adding the
/// per-run Basic-auth password (`server_password`) itself.
pub fn sidecar_url(state: &RuntimeState) -> Option<String> {
    state.lifecycle.lock().unwrap().url.clone()
}

fn xdg_config_home(env: &Env) -> Result<PathBuf, String> {
    Ok(runtime_root(env)?.join("xdg-config"))
}

/// The sidecar's XDG_DATA_HOME — also where the bundled goal plugin keeps its
/// per-session state (`opencode-goal-plugin/goals.json`, read by `goal.rs`).
pub fn xdg_data_home(env: &Env) -> Result<PathBuf, String> {
    Ok(runtime_root(env)?.join("xdg-data"))
}

/// File recording the user's chosen active workspace folder (absolute path).
fn active_workspace_file(env: &Env) -> Result<PathBuf, String> {
    Ok(runtime_root(env)?.join("active-workspace.txt"))
}

/// File recording the user's chosen BASE folder — it contains the managed
/// `projects/` and `sessions/` collections (Settings → Workspace).
fn base_workspace_file(env: &Env) -> Result<PathBuf, String> {
    Ok(runtime_root(env)?.join("base-workspace.txt"))
}

pub const PROJECTS_DIR_NAME: &str = "projects";
pub const SESSIONS_DIR_NAME: &str = "sessions";

/// Keep the user-visible workspace root predictable:
///
/// ```text
/// HappyScience/
///   projects/
///   sessions/
///   .openscience/
/// ```
///
/// Existing root-level workspaces are left where they are and remain readable.
/// Moving them would invalidate absolute session directories stored by OpenCode.
fn ensure_base_layout(dir: PathBuf) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for child in [PROJECTS_DIR_NAME, SESSIONS_DIR_NAME] {
        std::fs::create_dir_all(dir.join(child)).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

pub fn projects_dir(env: &Env) -> Result<PathBuf, String> {
    Ok(base_workspace_dir(env)?.join(PROJECTS_DIR_NAME))
}

pub fn sessions_dir(env: &Env) -> Result<PathBuf, String> {
    Ok(base_workspace_dir(env)?.join(SESSIONS_DIR_NAME))
}

/// A workspace path read back from disk. Installs predating #76 wrote the Windows
/// `\\?\` verbatim form here, which then flowed to the UI and could never match a
/// session's `directory`; unwrapping on read repairs them without a re-pick. The
/// transform is Windows-only — elsewhere `\` is a legal filename character.
fn persisted_path(raw: &str) -> String {
    #[cfg(target_os = "windows")]
    return crate::artifact_file::strip_windows_verbatim(raw);
    #[cfg(not(target_os = "windows"))]
    return raw.to_owned();
}

/// The active workspace folder OpenCode / the kernel / previews / provenance all
/// operate in. Defaults to the base folder (`~/Documents/HappyScience`) until the
/// user opens or creates another one; the choice persists across restarts.
pub fn workspace_dir(env: &Env) -> Result<PathBuf, String> {
    if let Ok(f) = active_workspace_file(env) {
        if let Ok(s) = std::fs::read_to_string(&f) {
            // Installs from before #76 persisted the Windows `\\?\` verbatim path;
            // unwrap it on read so those users are repaired without a re-pick.
            let dir = PathBuf::from(persisted_path(s.trim()));
            if dir.is_dir() {
                return ensure_base_layout(dir);
            }
        }
    }
    base_workspace_dir(env)
}

/// The workspace root containing the `projects/` and `sessions/` collections.
/// A folder the user picked in Settings wins; the default is `~/Documents/HappyScience`
/// (no space — the agent runs shell commands against this path, and unquoted
/// spaces break them), falling back to `$HOME/Documents`.
pub fn base_workspace_dir(env: &Env) -> Result<PathBuf, String> {
    if let Ok(f) = base_workspace_file(env) {
        if let Ok(s) = std::fs::read_to_string(&f) {
            let dir = PathBuf::from(persisted_path(s.trim()));
            if dir.is_dir() {
                return Ok(dir);
            }
        }
    }
    let docs = match env.document_dir() {
        Some(d) => d,
        None => {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map_err(|_| "could not resolve a documents directory".to_string())?;
            PathBuf::from(home).join("Documents")
        }
    };
    let dir = docs.join("HappyScience");

    // Keep the last shipped default in place: OpenCode stores absolute session
    // directories, so renaming OpenScience would orphan history. Fresh installs
    // use HappyScience; existing installs continue from their stable path.
    if !dir.exists() {
        let stable_legacy = docs.join("OpenScience");
        if stable_legacy.is_dir() {
            return ensure_base_layout(stable_legacy);
        }

        // Older pre-session layouts can still move. A failed rename (for
        // example cross-volume) keeps the existing location instead.
        for old in [
            docs.join("Open Science"),
            runtime_root(env)?.join("workspace"),
        ] {
            if old.is_dir() {
                if std::fs::rename(&old, &dir).is_ok() {
                    break;
                }
                return ensure_base_layout(old);
            }
        }
    }
    ensure_base_layout(dir)
}

/// Path OpenCode reads when XDG_CONFIG_HOME points at our private dir.
fn opencode_config_file(env: &Env) -> Result<PathBuf, String> {
    Ok(xdg_config_home(env)?.join("opencode").join("opencode.json"))
}

/// The config file to edit in place: the server may have rewritten the config
/// as opencode.jsonc — prefer whichever exists, fall back to opencode.json.
fn effective_config_file(env: &Env) -> Result<PathBuf, String> {
    let dir = xdg_config_home(env)?.join("opencode");
    Ok(["opencode.jsonc", "opencode.json"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
        .unwrap_or_else(|| dir.join("opencode.json")))
}

/// The user's existing OpenCode auth file (their login / free credits), if any.
/// Read-only: we copy it into our sandbox so the bundled runtime can use the same
/// login, but we never modify the user's file or sessions.
fn user_auth_source() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            candidates.push(PathBuf::from(xdg).join("opencode").join("auth.json"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(&home).join(".local/share/opencode/auth.json"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(PathBuf::from(appdata).join("opencode").join("auth.json"));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Copy the user's OpenCode CLI login into the app-private data dir, EXPLICITLY
/// (from the Settings page) — never silently. Returns false when there is no
/// CLI login to import. Restarts the sidecar so it picks the credentials up.
pub fn import_opencode_login(env: &Env) -> Result<bool, String> {
    let Some(src) = user_auth_source() else {
        return Ok(false);
    };
    let dst = runtime_root(env)?
        .join("xdg-data")
        .join("opencode")
        .join("auth.json");
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&src, &dst).map_err(|e| format!("copy failed: {e}"))?;

    // Restart the running sidecar so /config/providers reflects the login.
    restart_sidecar_if_running(env)?;
    Ok(true)
}

/// Whether the bundled runtime's credential store (its auth.json) has an entry
/// for this provider. The sidecar writes the token there the moment a browser
/// login completes, so the UI can fall back on it when the pending OAuth
/// callback request is lost (loopback port collision, proxy) — issue #17.
pub fn provider_auth_exists(env: &Env, provider_id: String) -> Result<bool, String> {
    let path = runtime_root(env)?
        .join("xdg-data")
        .join("opencode")
        .join("auth.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(false); // no store yet — no logins
    };
    Ok(auth_has_provider(&text, &provider_id))
}

fn auth_has_provider(text: &str, provider_id: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .is_some_and(|auth| auth.get(provider_id).is_some())
}

/// Deploy the bundled skill packs (Tauri resources) into the app-private
/// profile's global skills dir (`<xdg-config>/opencode/skills/`), which OpenCode
/// scans regardless of project detection: `skills/` is the external ai4s-skills
/// pack, `skills-office/` Anthropic's document skills (docx/pdf/pptx/xlsx),
/// `skills-agent-browser/` the version-matched official agent-browser skill,
/// `skills-core/` the first-party skills from `runtime/skills/core`. The
/// workspace's own `.opencode/skills/` stays reserved for skills the user
/// installs. Runs before every sidecar start so app upgrades refresh the packs.
fn deploy_bundled_skills(env: &Env) {
    let dst = match xdg_config_home(env) {
        Ok(cfg) => cfg.join("opencode").join("skills"),
        Err(_) => return,
    };
    let mut bundled: std::collections::HashSet<std::ffi::OsString> =
        std::collections::HashSet::new();
    let mut all_ok = true;
    for resource in [
        "skills",
        "skills-office",
        "skills-agent-browser",
        "skills-core",
    ] {
        let Some(src) = env.resource(resource).filter(|p| p.is_dir()) else {
            all_ok = false; // dev run without `fetch-skills.sh` — nothing to deploy
            continue;
        };
        match sync_skill_pack(&src, &dst) {
            Ok(names) => bundled.extend(names),
            Err(e) => {
                all_ok = false;
                eprintln!("failed to deploy bundled skills ({resource}): {e}");
            }
        }
    }
    // The global skills dir is exclusively app-managed (the user's own skills
    // live in the workspace's `.opencode/skills/`), so any skill dir not in the
    // freshly-bundled set is a stale leftover — e.g. one renamed across an app
    // upgrade (`hpc-slurm` → `remote-compute`) — and must be removed so the
    // obsolete duplicate can't shadow or confuse the agent. Prune ONLY when all
    // packs deployed cleanly: a partial deploy would make `bundled`
    // incomplete and wrongly delete valid skills.
    if all_ok {
        prune_stale_skills(&dst, &bundled);
        let missing = crate::capabilities::missing_skill_manifests(&dst);
        if !missing.is_empty() {
            eprintln!(
                "bundled research capability audit failed; missing skills: {}",
                missing.join(", ")
            );
        }
    }
}

const OPENCODE_PLUGIN_PACKAGE: &str = "@opencode-ai/plugin";

fn package_dependency_version(path: &Path, package: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("dependencies")?
        .get(package)?
        .as_str()
        .map(str::to_owned)
}

/// Does a package.json dependency SPEC name exactly this version?
///
/// The spec is a range — `npm install pkg@1.18.18` writes `^1.18.18` — while
/// the version bundled beside it is exact, so comparing the two as strings is
/// never equal. That is not a theoretical mismatch: it made
/// `deploy_goal_plugin_dependencies` fail its own post-check on every fresh
/// profile, so the goal plugin was never deployed and `/goal` silently did
/// nothing. Only a range that pins THIS version counts as a match; anything
/// wider or different is a genuine mismatch and still refuses.
fn dependency_pins(spec: &str, version: &str) -> bool {
    spec.trim()
        .trim_start_matches(['^', '~', '=', '>', '<', 'v', ' '])
        == version
}

/// Bring the app's OWN dependency line in step with the runtime it bundles.
///
/// A profile carries the version the app that CREATED it bundled — a July
/// profile still said `"@opencode-ai/plugin": "1.17.13"` — and the file above is
/// only ever copied when absent, so that line outlived the bump to 1.18.18. The
/// post-check then failed on every single start, `deploy_goal_plugin` returned
/// before its "refresh on every start" copy, and the profile kept whichever
/// plugin build it happened to receive first: an app upgrade could not deliver a
/// goal-plugin fix, and a profile that had never registered the plugin never got
/// `/goal` at all. Only this one dependency is rewritten — the user's other
/// plugin dependencies and every other key are written back untouched — and an
/// unparseable file is reported rather than replaced (#116).
fn adopt_bundled_plugin_dependency(package_json: &Path, bundled_spec: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(package_json).map_err(|e| e.to_string())?;
    let mut doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "{} is not readable JSON ({e}) — left as it is",
            package_json.display()
        )
    })?;
    let object = doc
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", package_json.display()))?;
    match object.get_mut("dependencies") {
        Some(serde_json::Value::Object(dependencies)) => {
            if dependencies
                .get(OPENCODE_PLUGIN_PACKAGE)
                .and_then(serde_json::Value::as_str)
                == Some(bundled_spec)
            {
                return Ok(());
            }
            dependencies.insert(
                OPENCODE_PLUGIN_PACKAGE.into(),
                serde_json::json!(bundled_spec),
            );
        }
        Some(_) => {
            return Err(format!(
                "{} has a \"dependencies\" that is not an object",
                package_json.display()
            ))
        }
        None => {
            object.insert(
                "dependencies".into(),
                serde_json::json!({ OPENCODE_PLUGIN_PACKAGE: bundled_spec }),
            );
        }
    }
    let mut out = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    out.push('\n');
    std::fs::write(package_json, out).map_err(|e| e.to_string())
}

/// Is this lockfile ours alone to replace? It is when it resolves no root
/// dependency other than the OpenCode plugin: a stale one pins the old version
/// (the July profile's lock named 1.17.13), and the next `npm install` would
/// resolve that right back over the tree just copied. A lockfile that also
/// holds the user's own packages is theirs, and is left exactly as it is.
fn lock_resolves_nothing_else(package_lock: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(package_lock) else {
        return false;
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    match doc
        .get("packages")
        .and_then(|p| p.get(""))
        .map(|root| root.get("dependencies"))
    {
        Some(Some(serde_json::Value::Object(dependencies))) => dependencies
            .keys()
            .all(|name| name == OPENCODE_PLUGIN_PACKAGE),
        // A lockfile with no root dependencies at all has nothing of the
        // user's in it to preserve.
        Some(None) | None => true,
        Some(Some(_)) => false,
    }
}

fn installed_package_version(node_modules: &Path, package: &str) -> Option<String> {
    let package_json = package
        .split('/')
        .fold(node_modules.to_path_buf(), |path, part| path.join(part))
        .join("package.json");
    let text = std::fs::read_to_string(package_json).ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}

/// Deploy OpenCode's plugin SDK before registering the bundled goal plugin.
/// OpenCode waits for this dependency before opening `/event`; without a local
/// copy, a fresh install performs a live npm install and an unreachable
/// registry leaves the desktop on "Connecting" for minutes.
fn deploy_goal_plugin_dependencies(src: &Path, dst: &Path) -> Result<(), String> {
    let expected = std::fs::read_to_string(src.join(".opencode-plugin-version"))
        .map_err(|_| "bundled goal plugin dependencies are missing".to_string())?;
    let expected = expected.trim();
    if expected.is_empty() {
        return Err("bundled OpenCode plugin version is empty".into());
    }

    let marker = dst.join(".opencode-plugin-version");
    let package_json = dst.join("package.json");
    let package_lock = dst.join("package-lock.json");
    let node_modules = dst.join("node_modules");
    let dependency_ready = package_dependency_version(&package_json, OPENCODE_PLUGIN_PACKAGE)
        .is_some_and(|spec| dependency_pins(&spec, expected))
        && installed_package_version(&node_modules, OPENCODE_PLUGIN_PACKAGE).as_deref()
            == Some(expected)
        && package_lock.is_file();
    let ready = dependency_ready
        && std::fs::read_to_string(&marker)
            .ok()
            .is_some_and(|v| v.trim() == expected);
    if ready {
        return Ok(());
    }
    // Existing app profiles may predate the marker but already have the exact
    // dependency from OpenCode's old live install. Adopt it without copying the
    // bundled 60 MB tree over the user's profile.
    if dependency_ready {
        return std::fs::write(marker, format!("{expected}\n")).map_err(|e| e.to_string());
    }

    let src_package = src.join("package.json");
    let src_lock = src.join("package-lock.json");
    let src_modules = src.join("node_modules");
    if !src_package.is_file() || !src_lock.is_file() || !src_modules.is_dir() {
        return Err("bundled OpenCode plugin dependency tree is incomplete".into());
    }

    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    copy_dir(&src_modules, &node_modules).map_err(|e| e.to_string())?;

    // A fresh app profile has neither file. Existing profiles created by
    // OpenCode already carry the same dependency; never overwrite a user's
    // additional plugin dependencies or lockfile.
    if !package_json.exists() {
        std::fs::copy(&src_package, &package_json).map_err(|e| e.to_string())?;
    } else {
        let bundled_spec = package_dependency_version(&src_package, OPENCODE_PLUGIN_PACKAGE)
            .ok_or("bundled goal plugin declares no OpenCode plugin dependency")?;
        adopt_bundled_plugin_dependency(&package_json, &bundled_spec)?;
    }
    if !package_lock.exists() || lock_resolves_nothing_else(&package_lock) {
        std::fs::copy(&src_lock, &package_lock).map_err(|e| e.to_string())?;
    }

    if !package_dependency_version(&package_json, OPENCODE_PLUGIN_PACKAGE)
        .is_some_and(|spec| dependency_pins(&spec, expected))
        || installed_package_version(&node_modules, OPENCODE_PLUGIN_PACKAGE).as_deref()
            != Some(expected)
    {
        return Err("OpenCode plugin dependency version does not match the bundled runtime".into());
    }
    std::fs::write(marker, format!("{expected}\n")).map_err(|e| e.to_string())
}

/// Ship the bundled goal plugin and its already-resolved OpenCode dependency
/// tree into the app-private profile, then return the plugin's absolute path.
/// None in dev runs without the fetch script.
fn deploy_goal_plugin(env: &Env) -> Option<PathBuf> {
    let resource = env.resource("goal-plugin").filter(|p| p.is_dir())?;
    let src = resource.join("goal-plugin.server.js");
    if !src.is_file() {
        return None;
    }
    let config_dir = xdg_config_home(env).ok()?.join("opencode");
    if let Err(e) = deploy_goal_plugin_dependencies(&resource, &config_dir) {
        eprintln!("failed to deploy goal plugin dependencies: {e}");
        return None;
    }
    let dst = config_dir.join("goal-plugin.server.js");
    std::fs::create_dir_all(&config_dir).ok()?;
    // Refresh on every start so app upgrades replace the plugin in place.
    if let Err(e) = std::fs::copy(&src, &dst) {
        eprintln!("failed to deploy goal plugin: {e}");
        return None;
    }
    Some(dst)
}

/// Deploy the dependency-free guard that removes model-supplied browser launch
/// overrides before OpenCode forwards a tool call to the official MCP server.
fn deploy_browser_guard_plugin(env: &Env) -> Option<PathBuf> {
    let src = env
        .resource("browser-plugin/browser-guard.ts")
        .filter(|p| p.is_file())?;
    let config_dir = xdg_config_home(env).ok()?.join("opencode");
    let dst = config_dir.join("browser-guard.ts");
    std::fs::create_dir_all(&config_dir).ok()?;
    if let Err(e) = std::fs::copy(&src, &dst) {
        eprintln!("failed to deploy browser guard plugin: {e}");
        return None;
    }
    Some(dst)
}

/// Ship app-owned custom tools into OpenCode's global tools directory. These
/// tools expose safe, declarative host capabilities (for example, asking the
/// UI to present an existing workspace artifact); they never hand the model a
/// raw window handle or filesystem access outside the active workspace.
fn deploy_workbench_tools(env: &Env) {
    let Some(src) = env.resource("tools").filter(|p| p.is_dir()) else {
        return;
    };
    let Ok(config_home) = xdg_config_home(env) else {
        return;
    };
    let dst = config_home.join("opencode").join("tools");
    if let Err(e) = std::fs::create_dir_all(&dst) {
        eprintln!("failed to create workbench tools directory: {e}");
        return;
    }
    let Ok(entries) = std::fs::read_dir(&src) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        if let Err(e) = std::fs::copy(&path, dst.join(name)) {
            eprintln!("failed to deploy workbench tool {}: {e}", path.display());
        }
    }
}

/// App-owned prompt files deployed into the OpenCode profile: the `reviewer`
/// agent and the commands that invoke it (#72). `(resource dir, profile dir)`.
const PROFILE_PROMPTS: &[(&str, &str)] =
    &[("profile/agent", "agent"), ("profile/command", "command")];

/// Ship the app's own agent and command definitions into the global profile
/// (`<xdg-config>/opencode/{agent,command}/`), which OpenCode scans for
/// `**/*.md` in every workspace. Refreshed on every sidecar start so app
/// upgrades replace them in place; files the user added themselves are left
/// alone, since only our own names are written.
fn deploy_profile_prompts(env: &Env) {
    let Ok(config_home) = xdg_config_home(env) else {
        return;
    };
    for (resource, dir) in PROFILE_PROMPTS {
        let Some(src) = env.resource(resource).filter(|p| p.is_dir()) else {
            continue; // dev run without the bundled resources
        };
        let dst = config_home.join("opencode").join(dir);
        if let Err(e) = std::fs::create_dir_all(&dst) {
            eprintln!("failed to create profile {dir} directory: {e}");
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&src) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension() != Some(std::ffi::OsStr::new("md")) {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            if let Err(e) = std::fs::copy(&path, dst.join(name)) {
                eprintln!("failed to deploy profile {dir} {}: {e}", path.display());
            }
        }
    }
}

/// Remove every SKILL.md-bearing directory in `dst` whose name is not in
/// `bundled` (the set just deployed). Non-skill directories — including the
/// reserved `user/` tree of installed skills — are left untouched.
fn prune_stale_skills(dst: &Path, bundled: &std::collections::HashSet<std::ffi::OsString>) {
    let Ok(entries) = std::fs::read_dir(dst) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == std::ffi::OsStr::new(USER_SKILLS_DIR) {
            continue;
        }
        if path.is_dir() && path.join("SKILL.md").is_file() && !bundled.contains(&entry.file_name())
        {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Copy every skill directory under `src` into `dst`, replacing same-named
/// directories (so bundled updates win) and leaving everything else in `dst`
/// alone. Returns the names of the skill directories it deployed (for stale
/// pruning). Directories without a SKILL.md (placeholders) are skipped.
fn sync_skill_pack(src: &Path, dst: &Path) -> std::io::Result<Vec<std::ffi::OsString>> {
    std::fs::create_dir_all(dst)?;
    let mut deployed = Vec::new();
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || !entry.path().join("SKILL.md").is_file() {
            continue;
        }
        // `user/` belongs to the installed skills — a pack may never claim it.
        if entry.file_name() == std::ffi::OsStr::new(USER_SKILLS_DIR) {
            continue;
        }
        let target = dst.join(entry.file_name());
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        copy_dir(&entry.path(), &target)?;
        deployed.push(entry.file_name());
    }
    Ok(deployed)
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Reserved subdirectory of the profile's global skills dir holding the skills
/// the USER installs. It lives inside a directory OpenCode already scans
/// (`<xdg-config>/opencode/skills/`, matched recursively by both skill loaders),
/// so an installed skill is available in EVERY workspace — a session's own
/// `.opencode/skills/` would vanish with the next dated session folder (#61).
/// Bundled-pack pruning skips this name, so app upgrades never delete it.
const USER_SKILLS_DIR: &str = "user";

fn user_skills_dir(env: &Env) -> Result<PathBuf, String> {
    Ok(xdg_config_home(env)?
        .join("opencode")
        .join("skills")
        .join(USER_SKILLS_DIR))
}

/// The `name:` from a SKILL.md's YAML frontmatter, or None when the text is not
/// a skill file (no leading frontmatter, no usable name).
fn skill_name_from_markdown(text: &str) -> Option<String> {
    let front = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .strip_prefix("---")?
        .split_once("\n---")?
        .0
        .to_string();
    front.lines().find_map(|line| {
        let value = line.trim().strip_prefix("name:")?;
        sanitize_skill_name(value.trim().trim_matches(['"', '\'']))
    })
}

/// A skill's name doubles as its directory name — accept only what cannot
/// escape the skills dir (no separators, no `..`, no hidden names).
fn sanitize_skill_name(name: &str) -> Option<String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then(|| name.to_string())
}

/// Skill directories in the active workspace's `.opencode/skills/`.
fn workspace_skill_dirs(workspace: &Path) -> Vec<PathBuf> {
    let root = workspace.join(".opencode").join("skills");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("SKILL.md").is_file())
        .collect()
}

fn dir_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|n| n.to_str())
}

/// Install a pasted SKILL.md straight into the profile's user skills dir — no
/// model turn, no provider needed — and restart the sidecar so OpenCode
/// rediscovers it (discovery is cached per instance). Returns the skill's name.
pub fn install_skill_markdown(env: &Env, text: String) -> Result<String, String> {
    let name = skill_name_from_markdown(&text)
        .ok_or_else(|| "not a skill file: it needs YAML frontmatter with a `name:`".to_string())?;
    // A bundled pack owns its name: two skills sharing one name make OpenCode
    // pick whichever it scanned last, so refuse instead of shadowing.
    if xdg_config_home(env)?
        .join("opencode")
        .join("skills")
        .join(&name)
        .join("SKILL.md")
        .is_file()
    {
        return Err(format!("a bundled skill is already called \"{name}\""));
    }
    let dir = user_skills_dir(env)?.join(&name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("SKILL.md"), text.as_bytes()).map_err(|e| e.to_string())?;
    restart_sidecar_if_running(env)?;
    Ok(name)
}

/// Names already in the workspace's `.opencode/skills/`. Taken before an agent
/// install runs so `adopt_workspace_skills` can tell what it added.
pub fn workspace_skill_names(env: &Env) -> Result<Vec<String>, String> {
    Ok(workspace_skill_dirs(&workspace_dir(env)?)
        .iter()
        .filter_map(|p| dir_name(p).map(str::to_owned))
        .collect())
}

/// Move skills the agent just wrote into the workspace over to the profile's
/// user skills dir, so they outlive that session's folder. `known` is the
/// pre-install listing — a pinned project's own skills stay project-scoped.
/// The workspace copy is dropped once the profile copy is in place: leaving both
/// would give OpenCode two skills with the same name, and it then picks whichever
/// it scanned last. Restarts the sidecar when anything moved; returns the names.
pub fn adopt_workspace_skills(env: &Env, known: Vec<String>) -> Result<Vec<String>, String> {
    let dst_root = user_skills_dir(env)?;
    let mut adopted = Vec::new();
    for src in workspace_skill_dirs(&workspace_dir(env)?) {
        let Some(name) = dir_name(&src) else { continue };
        if known.iter().any(|k| k == name) || sanitize_skill_name(name).is_none() {
            continue;
        }
        let dst = dst_root.join(name);
        if dst.exists() {
            std::fs::remove_dir_all(&dst).map_err(|e| e.to_string())?;
        }
        copy_dir(&src, &dst).map_err(|e| e.to_string())?;
        // Only now that the profile copy exists — a failed cleanup leaves a
        // harmless duplicate, never a lost skill.
        if let Err(e) = std::fs::remove_dir_all(&src) {
            eprintln!("could not remove the workspace copy of {name}: {e}");
        }
        adopted.push(name.to_string());
    }
    if !adopted.is_empty() {
        restart_sidecar_if_running(env)?;
    }
    Ok(adopted)
}

/// PATH for the sidecar (and everything the agent runs through it). Apps
/// launched from Finder/Dock/a desktop entry get a minimal PATH, so the agent
/// would not find the user's Python/conda/Homebrew tools. Prepend the
/// well-known locations that actually exist — the platform lists differ
/// (macOS Homebrew vs. Linux /opt/conda & Linuxbrew), same as python_candidates.
#[cfg(unix)]
pub fn enriched_path() -> String {
    let base = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    #[cfg(target_os = "macos")]
    let extras = [
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        format!("{home}/anaconda3/bin"),
        format!("{home}/miniconda3/bin"),
        "/opt/anaconda3/bin".to_string(),
        "/opt/miniconda3/bin".to_string(),
        format!("{home}/.pyenv/shims"),
        format!("{home}/.local/bin"),
    ];
    #[cfg(target_os = "linux")]
    let extras = [
        format!("{home}/anaconda3/bin"),
        format!("{home}/miniconda3/bin"),
        "/opt/conda/bin".to_string(),
        "/opt/anaconda3/bin".to_string(),
        "/opt/miniconda3/bin".to_string(),
        format!("{home}/.pyenv/shims"),
        "/home/linuxbrew/.linuxbrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        format!("{home}/.local/bin"),
    ];
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
    let extras = [
        format!("{home}/.pyenv/shims"),
        "/usr/local/bin".to_string(),
        format!("{home}/.local/bin"),
    ];

    let mut parts: Vec<String> = extras
        .into_iter()
        .filter(|p| !base.split(':').any(|b| b == p) && std::path::Path::new(p).is_dir())
        .collect();
    if !base.is_empty() {
        parts.push(base);
    }
    parts.join(":")
}

/// Windows twin of the unix version above: GUI apps inherit a PATH without the
/// user's Python/conda, and Anaconda famously does NOT add itself to PATH.
/// Prepend the conda install roots that exist — including `Library\bin`, which
/// conda pythons need on PATH for their DLLs (numpy fails to import otherwise).
#[cfg(windows)]
pub fn enriched_path() -> String {
    let base = std::env::var("PATH").unwrap_or_default();
    let mut roots: Vec<String> = Vec::new();
    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(format!("{profile}\\anaconda3"));
        roots.push(format!("{profile}\\miniconda3"));
    }
    roots.push("C:\\ProgramData\\anaconda3".into());
    roots.push("C:\\ProgramData\\miniconda3".into());
    let mut extras: Vec<String> = Vec::new();
    for root in roots {
        for dir in [
            root.clone(),
            format!("{root}\\Scripts"),
            format!("{root}\\Library\\bin"),
        ] {
            extras.push(dir);
        }
    }
    let mut parts: Vec<String> = extras
        .into_iter()
        .filter(|p| !base.split(';').any(|b| b.eq_ignore_ascii_case(p)) && Path::new(p).is_dir())
        .collect();
    if !base.is_empty() {
        parts.push(base);
    }
    parts.join(";")
}

const BUNDLED_SIDECAR_PREFIX: &str = "happy-science-";

fn sidecar_filename(name: &str) -> String {
    let name = format!("{BUNDLED_SIDECAR_PREFIX}{name}");
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name
    }
}

/// On-disk path of a bundled sidecar (`externalBin`), if it is there. Tauri
/// places them next to the app executable with the target-triple suffix
/// stripped. The product prefix prevents Linux packages from claiming common
/// global commands such as `uv` and `opencode` under `/usr/bin`.
pub fn sidecar_bin(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let file = sidecar_filename(name);
    let bin = exe.parent()?.join(file);
    bin.exists().then_some(bin)
}

/// A `std::process::Command` that never pops a console window on Windows.
/// A GUI app spawning a console-subsystem child (python.exe, taskkill, git…)
/// otherwise flashes a black window per spawn — every direct spawn in this
/// crate must go through here. (Sidecars via tauri_plugin_shell already set
/// the flag internally.)
pub fn quiet_command(bin: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Spawn the sidecar so the kernel takes it with us if we die uncatchably.
///
/// Measured on a headless Ubuntu server: `kill -9` on the parent (or an OOM
/// kill) left OpenCode running and reparented to init, still holding the port
/// and the session database, with nothing left to shut it down — no handler of
/// ours runs for SIGKILL. systemd hides this by killing the whole cgroup, so the
/// leak only bit outside a unit (nohup, tmux, a bare shell).
///
/// `PR_SET_PDEATHSIG` fixes it, with one trap that makes the obvious
/// implementation worse than the bug: the signal fires when the THREAD that
/// created the child exits, not when the process does. Sidecars are spawned from
/// short-lived threads all the time — a gateway request handler restarting the
/// runtime, a Tauri command on a pool thread — and each of those would kill the
/// sidecar seconds after starting it. So every spawn goes through one thread
/// that is created once and never returns.
#[cfg(target_os = "linux")]
fn spawn_tied_to_our_lifetime(mut cmd: std::process::Command) -> std::io::Result<Child> {
    use std::sync::mpsc::{channel, Sender};
    type Request = (std::process::Command, Sender<std::io::Result<Child>>);
    static SPAWNER: std::sync::OnceLock<Mutex<Sender<Request>>> = std::sync::OnceLock::new();

    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // PR_SET_PDEATHSIG = 1, SIGKILL = 9 — two integers, so declared here
            // rather than taking a dependency on libc.
            const PR_SET_PDEATHSIG: i32 = 1;
            extern "C" {
                fn prctl(option: i32, ...) -> i32;
            }
            if prctl(PR_SET_PDEATHSIG, 9) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let spawner = SPAWNER.get_or_init(|| {
        let (tx, rx) = channel::<Request>();
        std::thread::Builder::new()
            .name("sidecar-spawner".into())
            .spawn(move || {
                // Lives as long as the process, which is the whole point: the
                // parent-death signal is bound to this thread.
                for (mut cmd, reply) in rx {
                    let _ = reply.send(cmd.spawn());
                }
            })
            .expect("spawn the sidecar spawner thread");
        Mutex::new(tx)
    });

    let (reply_tx, reply_rx) = channel();
    spawner
        .lock()
        .map_err(|_| std::io::Error::other("the sidecar spawner lock was poisoned"))?
        .send((cmd, reply_tx))
        .map_err(|_| std::io::Error::other("the sidecar spawner thread is gone"))?;
    reply_rx
        .recv()
        .map_err(|_| std::io::Error::other("the sidecar spawner thread stopped answering"))?
}

/// Everywhere else this is just a spawn: macOS has no equivalent of
/// PR_SET_PDEATHSIG, and Windows uses the console handler installed in
/// `osd server` plus the job the desktop app runs in.
#[cfg(not(target_os = "linux"))]
fn spawn_tied_to_our_lifetime(mut cmd: std::process::Command) -> std::io::Result<Child> {
    cmd.spawn()
}

/// Make a secret-holding path owner-only: 700 for directories, 600 for files
/// (unix). The runtime root carries provider/connector API keys in
/// `opencode.jsonc`/`auth.json`, and the sidecar rewrites those files with a
/// default umask while running — locking the DIRECTORY is what holds, since a
/// 700 dir is unreachable for other users whatever the file modes inside. On
/// Windows, %APPDATA% is per-user ACL'd already; nothing to do.
pub fn tighten_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = if meta.is_dir() { 0o700 } else { 0o600 };
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// `bytes` bytes of OS randomness as lowercase hex. Panics only if the OS
/// CSPRNG is unavailable — a machine state where serving anything is unsafe.
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS random source unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Per-run password the sidecar requires on every HTTP request (OpenCode's
/// built-in Basic auth, `OPENCODE_SERVER_PASSWORD`). Generated fresh each app
/// launch and held only in memory — never written to disk — so a local
/// webpage that scans loopback ports can neither drive agent turns nor read
/// `/global/config` (which carries provider API keys). The webview gets it
/// via the `runtime_password` command; Tauri IPC is app-only.
pub fn server_password() -> &'static str {
    static PASSWORD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PASSWORD.get_or_init(|| random_hex(16))
}

/// Expose the per-run sidecar password to the frontend SDK client.
pub fn runtime_password() -> String {
    server_password().to_string()
}

pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(43917)
}

/// Network-proxy setting for the sidecar: `system` (default) follows the OS,
/// `custom <url>` uses a fixed proxy, `none` forces direct connections.
/// Stored as one line in `proxy.txt` under the runtime root.
fn proxy_setting_file(env: &Env) -> Result<PathBuf, String> {
    Ok(runtime_root(env)?.join("proxy.txt"))
}

/// The persisted proxy setting as (mode, url). Unknown/missing → system.
fn read_proxy_setting(env: &Env) -> (String, String) {
    let raw = proxy_setting_file(env)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let line = raw.lines().next().unwrap_or("").trim();
    match line.split_once(' ') {
        Some(("custom", url)) if !url.trim().is_empty() => ("custom".into(), url.trim().into()),
        _ if line == "none" => ("none".into(), String::new()),
        _ => ("system".into(), String::new()),
    }
}

/// Accept `http://`, `https://` or `socks5://` with a host:port.
fn validate_proxy_url(url: &str) -> Result<(), String> {
    let rest = ["http://", "https://", "socks5://"]
        .iter()
        .find_map(|s| url.strip_prefix(s))
        .ok_or("proxy URL must start with http://, https:// or socks5://")?;
    let hostport = rest.trim_end_matches('/');
    let (host, port) = hostport
        .rsplit_once(':')
        .ok_or("proxy URL needs a host:port")?;
    if host.is_empty() || port.parse::<u16>().is_err() {
        return Err("proxy URL needs a host:port".into());
    }
    Ok(())
}

/// Proxy env for the sidecar. A GUI app launched from Finder/Dock inherits no
/// shell environment, so a user whose traffic runs through a system proxy
/// (common where provider hosts are unreachable directly) gets a sidecar that
/// cannot reach them: its fetch honors HTTP(S)_PROXY but nothing sets it.
/// Resolved from the persisted setting: `system` mirrors the OS proxy (an
/// existing env always wins — a terminal launch already carries the user's own
/// values), `custom` pins the user's URL, `none` neutralizes even inherited
/// env. Verified live with xAI OAuth (#9): the proxied browser delivers the
/// code, then the sidecar's token exchange to auth.x.ai hangs without a proxy
/// and succeeds with one.
fn resolve_proxy_env(mode: &str, url: &str) -> Vec<(&'static str, String)> {
    // Loopback traffic (the sidecar's own API, provider OAuth callback
    // servers) must never route through a proxy.
    const NO_PROXY_LOOPBACK: &str = "localhost,127.0.0.1,::1";
    match mode {
        "none" => vec![
            ("HTTP_PROXY", String::new()),
            ("HTTPS_PROXY", String::new()),
            ("http_proxy", String::new()),
            ("https_proxy", String::new()),
            ("ALL_PROXY", String::new()),
            ("NO_PROXY", "*".to_string()),
        ],
        "custom" => vec![
            ("HTTP_PROXY", url.to_string()),
            ("HTTPS_PROXY", url.to_string()),
            ("NO_PROXY", NO_PROXY_LOOPBACK.to_string()),
        ],
        _ => {
            if ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
                .iter()
                .any(|k| std::env::var_os(k).is_some())
            {
                return Vec::new();
            }
            match system_proxy_url() {
                Some(sys) => vec![
                    ("HTTP_PROXY", sys.clone()),
                    ("HTTPS_PROXY", sys),
                    ("NO_PROXY", NO_PROXY_LOOPBACK.to_string()),
                ],
                None => Vec::new(),
            }
        }
    }
}

/// The proxy the sidecar would actually use right now, for display in
/// Settings. None ⇒ direct connections.
fn effective_proxy(mode: &str, url: &str) -> Option<String> {
    match mode {
        "none" => None,
        "custom" => Some(url.to_string()),
        _ => ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .or_else(system_proxy_url),
    }
}

/// PyPI-index and Python-download mirrors for the bundled uv, stored one per
/// line (`pypi <url>` / `python <url>`) in `mirrors.txt` under the runtime root.
/// Empty ⇒ uv's defaults (pypi.org / github.com). Only the uv provisioning
/// flows read these — no long-running sidecar to restart.
fn mirror_setting_file(env: &Env) -> Result<PathBuf, String> {
    Ok(runtime_root(env)?.join("mirrors.txt"))
}

/// The persisted mirrors as (pypi_index_url, python_install_mirror_url).
fn read_mirror_setting(env: &Env) -> (String, String) {
    let raw = mirror_setting_file(env)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let (mut pypi, mut python) = (String::new(), String::new());
    for line in raw.lines() {
        match line.trim().split_once(' ') {
            Some(("pypi", v)) => pypi = v.trim().to_string(),
            Some(("python", v)) => python = v.trim().to_string(),
            _ => {}
        }
    }
    (pypi, python)
}

/// Accept an `http(s)://` URL with a non-empty host.
fn validate_mirror_url(url: &str) -> Result<(), String> {
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|s| url.strip_prefix(s))
        .ok_or("mirror URL must start with http:// or https://")?;
    if rest.trim_matches('/').is_empty() {
        return Err("mirror URL needs a host".into());
    }
    Ok(())
}

/// Network env for the bundled uv sidecar (managed-Python download + pip
/// install). Mirrors the OpenCode sidecar's proxy so first-run provisioning
/// works behind the same proxy the agent uses, and adds the optional PyPI /
/// Python-download mirrors. uv reads HTTP(S)_PROXY, `UV_DEFAULT_INDEX` and
/// `UV_PYTHON_INSTALL_MIRROR` from its environment.
pub fn uv_network_env(env: &Env) -> Vec<(&'static str, String)> {
    let (mode, url) = read_proxy_setting(env);
    let mut vars = resolve_proxy_env(&mode, &url);
    let (pypi, python) = read_mirror_setting(env);
    if !pypi.is_empty() {
        vars.push(("UV_DEFAULT_INDEX", pypi));
    }
    if !python.is_empty() {
        vars.push(("UV_PYTHON_INSTALL_MIRROR", python));
    }
    vars
}

/// Proxy env for a bundled sidecar OTHER than opencode (e.g. agent-browser's
/// Chrome download). Same resolution as the OpenCode sidecar so a first-run
/// browser install works behind the user's configured proxy, without the uv
/// mirror vars that only uv understands.
pub fn sidecar_proxy_env(env: &Env) -> Vec<(&'static str, String)> {
    let (mode, url) = read_proxy_setting(env);
    resolve_proxy_env(&mode, &url)
}

/// The system-configured proxy as a URL, if one is enabled (macOS: scutil).
/// HTTP(S) proxies are preferred — an HTTPS proxy endpoint still speaks plain
/// HTTP CONNECT, hence the http:// scheme — with SOCKS as the fallback.
#[cfg(target_os = "macos")]
fn system_proxy_url() -> Option<String> {
    let out = quiet_command("scutil").arg("--proxy").output().ok()?;
    parse_scutil_proxy(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `scutil --proxy` output (`  Key : value` lines) into a proxy URL.
/// Compiled where it is reachable — macOS — plus tests everywhere, so the
/// parser stays covered on any host without warning as dead code on the ones
/// that never call it. (Same shape as `strip_windows_verbatim`.)
#[cfg(any(target_os = "macos", test))]
fn parse_scutil_proxy(text: &str) -> Option<String> {
    let get = |key: &str| -> Option<String> {
        let prefix = format!("{key} : ");
        text.lines().find_map(|l| {
            l.trim()
                .strip_prefix(prefix.as_str())
                .map(|v| v.trim().to_string())
        })
    };
    let enabled = |key: &str| get(key).as_deref() == Some("1");
    for (en, host, port, scheme) in [
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort", "http"),
        ("HTTPEnable", "HTTPProxy", "HTTPPort", "http"),
        ("SOCKSEnable", "SOCKSProxy", "SOCKSPort", "socks5"),
    ] {
        if enabled(en) {
            if let (Some(h), Some(p)) = (get(host), get(port)) {
                return Some(format!("{scheme}://{h}:{p}"));
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn system_proxy_url() -> Option<String> {
    // Windows/Linux: terminal-launched apps inherit the user's proxy env
    // (covered by the passthrough above); no OS store is read here yet.
    None
}

/// One-time upgrade cleanup for connector configs created before the app gave
/// the agent browser its own namespace: close the daemon started under the old,
/// shared one. Spawned and not waited on — a stuck helper must never hold up a
/// sidecar start. A no-op where agent-browser is not bundled (a headless run).
fn close_legacy_agent_browser() {
    let Some(bin) = sidecar_bin("agent-browser") else {
        return;
    };
    let _ = quiet_command(bin).args(["close", "--all"]).spawn();
}

fn spawn_sidecar(env: &Env, port: u16, generation: u64) -> Result<Child, String> {
    let root = runtime_root(env)?;
    let cfg = root.join("xdg-config");
    let data = root.join("xdg-data");
    let cache = root.join("xdg-cache");
    let state_dir = root.join("xdg-state");
    // Run OpenCode inside the user-facing workspace, NOT the app's cwd (which is `/`
    // when launched from Finder) — otherwise it scans the whole filesystem root.
    let workspace = workspace_dir(env)?;
    for d in [&cfg, &data, &cache, &state_dir] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    // Ship the bundled scientific skills into the app-private OpenCode profile.
    deploy_bundled_skills(env);
    // Host presentation tools are global to the app-owned OpenCode profile and
    // available in every session workspace.
    deploy_workbench_tools(env);
    // The reviewer agent and its commands, same profile, same refresh-on-start.
    deploy_profile_prompts(env);
    // Safety default (AGENTS.md non-negotiable): on first run, seed the
    // "approve" permission mode so dangerous shell commands prompt for
    // approval. A mode the user chose (approve or full) is never overridden.
    let cfg_file = effective_config_file(env)?;
    let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
    if let Some(seeded) = crate::opencode_config::seed_default_permission(&existing) {
        if let Some(dir) = cfg_file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&cfg_file, seeded).map_err(|e| e.to_string())?;
    }
    // Installs that chose a mode before this app wrote any `external_directory`
    // rules kept OpenCode's builtin ask on every path outside the workspace —
    // the switch alone would never have refreshed them, since a chosen mode is
    // never re-seeded. Back-fill once; it is additive and idempotent.
    let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
    if let Some(migrated) = crate::opencode_config::migrate_external_directory(&existing) {
        std::fs::write(&cfg_file, migrated).map_err(|e| e.to_string())?;
    }
    // Same reason, same one-time shape: approve mode gained a browser ask rule
    // after these installs picked their mode.
    let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
    if let Some(migrated) = crate::opencode_config::migrate_browser_permission(&existing) {
        std::fs::write(&cfg_file, migrated).map_err(|e| e.to_string())?;
    }
    // Rename the legacy browser MCP id, then hide the incompatible user skill
    // with that old name while the official connector is configured.
    let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
    let close_legacy_browser = crate::opencode_config::browser_uses_legacy_namespace(&existing);
    if let Some(migrated) = crate::opencode_config::migrate_browser_integration(&existing) {
        std::fs::write(&cfg_file, migrated).map_err(|e| e.to_string())?;
        if close_legacy_browser {
            close_legacy_agent_browser();
        }
    }
    // Existing installs called agent-browser directly. Put the first-party MCP
    // ownership boundary in front of it before OpenCode starts, preserving the
    // user's selected upstream tool profile.
    if let (Ok(proxy_bin), Some(agent_browser_bin)) =
        (std::env::current_exe(), sidecar_bin("agent-browser"))
    {
        let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
        let proxy = proxy_bin.to_string_lossy().replace('\\', "/");
        let agent = agent_browser_bin.to_string_lossy().replace('\\', "/");
        if let Some(updated) =
            crate::opencode_config::ensure_browser_mcp_proxy(&existing, &proxy, &agent)
        {
            std::fs::write(&cfg_file, updated).map_err(|e| e.to_string())?;
        }
    }
    // Long conversations must not die on "Input exceeds context window" (#62):
    // turn OpenCode's automatic compaction on for a config that has never
    // said either way, and register the memory layers (global MEMORY.md +
    // each project's own AGENTS.md) the same one-time way. Both respect a
    // later choice by the user — they only seed what is absent.
    {
        let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
        if let Some(updated) = crate::opencode_config::seed_compaction(&existing) {
            std::fs::write(&cfg_file, updated).map_err(|e| e.to_string())?;
        }
        let global_memory = global_memory_file(env)?
            .to_string_lossy()
            .replace('\\', "/");
        let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
        // Absent `instructions` means a fresh profile: switch memory on. A
        // config that already lists instructions is the user's, left alone.
        let untouched = serde_json::from_str::<serde_json::Value>(&existing)
            .ok()
            .is_none_or(|v| v.get("instructions").is_none());
        if untouched {
            if let Some(updated) =
                crate::opencode_config::set_memory_enabled(&existing, &global_memory, true)
            {
                std::fs::write(&cfg_file, updated).map_err(|e| e.to_string())?;
            }
        }
    }
    // Goal mode (/goal): register the bundled plugin under its deployed path.
    // Forward slashes everywhere — Windows accepts them, and the config stays
    // portable for opencode's path-spec detection.
    if let Some(plugin_path) = deploy_goal_plugin(env) {
        let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
        let path_str = plugin_path.to_string_lossy().replace('\\', "/");
        if let Some(updated) = crate::opencode_config::ensure_goal_plugin(&existing, &path_str) {
            std::fs::write(&cfg_file, updated).map_err(|e| e.to_string())?;
        }
    }
    // Browser launch/session policy is an app invariant, not a model hint.
    if let Some(plugin_path) = deploy_browser_guard_plugin(env) {
        let existing = std::fs::read_to_string(&cfg_file).unwrap_or_default();
        let path_str = plugin_path.to_string_lossy().replace('\\', "/");
        if let Some(updated) =
            crate::opencode_config::ensure_browser_guard_plugin(&existing, &path_str)
        {
            std::fs::write(&cfg_file, updated).map_err(|e| e.to_string())?;
        }
    }
    // Secrets live under the runtime root (provider/connector keys in
    // opencode.jsonc, OpenCode's auth.json) — owner-only on every start, so
    // existing installs are repaired and whatever the sidecar later rewrites
    // inside stays unreachable to other users regardless of its umask.
    tighten_private(&root);
    tighten_private(&cfg_file);
    let home = std::env::var("HOME").unwrap_or_default();
    let port_str = port.to_string();

    let bin = sidecar_bin("opencode")
        .ok_or_else(|| "bundled OpenCode binary not found next to the executable".to_string())?;

    let mut cmd = quiet_command(bin);
    cmd.args([
        "serve",
        "--hostname",
        "127.0.0.1",
        "--port",
        port_str.as_str(),
    ])
    // Require auth on every request (P0-7): without a password the server
    // trusts ANY localhost-origin page (verified in the 1.17.13 source —
    // its CORS allowlist admits http://localhost:*/127.0.0.1:* wholesale,
    // and `--cors "*"` was only ever an exact-match literal, not a
    // wildcard). The webview authenticates via the SDK; nothing else may.
    .env("OPENCODE_SERVER_PASSWORD", server_password())
    // App-private dirs: OpenCode never touches the user's ~/.config/opencode.
    .env("XDG_CONFIG_HOME", cfg.to_string_lossy().to_string())
    .env("XDG_DATA_HOME", data.to_string_lossy().to_string())
    .env("XDG_CACHE_HOME", cache.to_string_lossy().to_string())
    .env("XDG_STATE_HOME", state_dir.to_string_lossy().to_string())
    .env("HOME", home)
    // Lets bundled skill helpers (e.g. remote-compute's record_run.py) stamp
    // the recording app version into provenance — they run outside the app
    // and can't otherwise know it.
    .env("OPENSCIENCE_APP_VERSION", env.version())
    // GUI-launched apps get a minimal PATH; give the agent the user's real tools.
    .env("PATH", enriched_path())
    .current_dir(workspace)
    // OpenCode keeps its own log file (xdg-data/opencode/log/opencode.log),
    // so its stdout carries nothing worth a pipe — and an undrained pipe
    // would eventually block the process. stderr IS drained, below.
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped());
    // The agent's own `ssh`/`rsync`/`sbatch` calls ride the app's shared
    // connection through this config, so a host the user signed in to once needs
    // no further password or one-time code (#73). The bundled remote-compute
    // skill and the ssh_connect tool both pass `-F "$OPENSCIENCE_SSH_CONFIG"`.
    if let Some(ssh_config) = ssh_config_path(env) {
        cmd.env(
            "OPENSCIENCE_SSH_CONFIG",
            ssh_config.to_string_lossy().to_string(),
        );
    }
    // Apply the network-proxy setting so provider logins and API calls work
    // where direct connections are blocked (see resolve_proxy_env).
    let (proxy_mode, proxy_url) = read_proxy_setting(env);
    for (k, v) in resolve_proxy_env(&proxy_mode, &proxy_url) {
        cmd.env(k, v);
    }

    let mut child =
        spawn_tied_to_our_lifetime(cmd).map_err(|e| format!("failed to spawn opencode: {e}"))?;
    // Drain stderr so the child's pipe never fills, AND record the failure
    // signals we used to discard. When the ad-hoc-signed sidecar dies during
    // bootstrap (TCC denial, config-merge abort, panic) the only symptom was a
    // generic "Could not open OpenCode event stream" in the UI with no cause.
    // Now stderr and the exit status land in debug.log next to the frontend's
    // connection attempts.
    //
    // stderr reaching EOF means the process is gone, which is also how the
    // lifecycle learns a sidecar died: without that, the handle and URL stayed
    // behind, `start_runtime`'s "already running" early return handed the
    // frontend the URL of a process that no longer existed, and the app retried
    // that dead port forever — a crash the runtime could have recovered from in
    // seconds instead needed the whole app restarted. The port is kept: it is
    // free again, and reusing it means the frontend's URL survives the respawn.
    if let Some(stderr) = child.stderr.take() {
        let env = env.clone();
        std::thread::spawn(move || {
            drain_stderr(&env, stderr);
            let status = {
                let mut lifecycle = match env.runtime().lifecycle.lock() {
                    Ok(l) => l,
                    Err(_) => return,
                };
                if lifecycle.generation != generation {
                    return; // a later sidecar owns the lifecycle now
                }
                let status = lifecycle.child.take().and_then(|mut c| c.wait().ok());
                lifecycle.url = None;
                status
            };
            crate::debug_log::append(
                &env,
                &format!("[opencode] terminated: {}", describe_exit(status)),
            );
        });
    }
    Ok(child)
}

/// Read a sidecar's stderr to EOF, one log line at a time.
///
/// Splits on `\r` as well as `\n`, and holds raw BYTES until a delimiter
/// arrives rather than decoding each read: a progress line that ends in a bare
/// carriage return would otherwise never be flushed (and would grow a String
/// without bound), and decoding per chunk mangles any UTF-8 sequence that
/// straddles a read boundary — a real risk here, since agent output is
/// routinely non-ASCII. `LINE_CAP` bounds the pathological case of a process
/// that writes megabytes with no delimiter at all.
fn drain_stderr<R: std::io::Read>(env: &Env, stderr: R) {
    use std::io::Read;
    const LINE_CAP: usize = 64 * 1024;

    let mut reader = std::io::BufReader::new(stderr);
    let mut chunk = [0u8; 4096];
    let mut pending: Vec<u8> = Vec::new();
    let emit = |bytes: &[u8]| {
        let line = String::from_utf8_lossy(bytes);
        let line = line.trim();
        if !line.is_empty() {
            crate::debug_log::append(env, &format!("[opencode] {line}"));
        }
    };
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &byte in &chunk[..read] {
            if byte == b'\n' || byte == b'\r' {
                emit(&pending);
                pending.clear();
            } else {
                pending.push(byte);
                if pending.len() >= LINE_CAP {
                    emit(&pending);
                    pending.clear();
                }
            }
        }
    }
    emit(&pending);
}

/// Kill a sidecar and REAP it.
///
/// `std::process::Child::kill` only sends the signal; a child that is never
/// waited on stays a zombie until this process exits. The desktop restarts the
/// sidecar on every approval-mode, provider, agent-model and skill change, and
/// the reconnect loop forces one after 8 failures — so "one zombie per restart"
/// is a leak that accumulates over a working day. (`tauri_plugin_shell`'s
/// CommandChild::kill waited internally, which is why nothing needed this
/// before.) The wait returns at once: the process is already dying.
fn kill_and_reap(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// How a sidecar ended, for the log: an exit code, or the signal that killed it.
fn describe_exit(status: Option<std::process::ExitStatus>) -> String {
    let Some(status) = status else {
        return "status unavailable".into();
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal={signal}");
        }
    }
    match status.code() {
        Some(code) => format!("code={code}"),
        None => "status unavailable".into(),
    }
}

/// The shared ssh config, when the desktop has written one. Generating it needs
/// the user's own `~/.ssh/config` and a private control directory, which is the
/// desktop's job (`ssh_session`); the sidecar only ever needs the path.
fn ssh_config_path(env: &Env) -> Option<PathBuf> {
    let path = runtime_root(env).ok()?.join("ssh").join("config");
    path.is_file().then_some(path)
}

/// Kill and respawn a running sidecar on its stable port. The lifecycle lock
/// covers the complete state transition, and URL is cleared before spawning so
/// a failed restart can never leave a stale "running" marker behind.
fn restart_sidecar_if_running(env: &Env) -> Result<Option<String>, String> {
    let mut lifecycle = env.runtime().lifecycle.lock().unwrap();
    let Some(child) = lifecycle.child.take() else {
        lifecycle.url = None;
        return Ok(None);
    };
    lifecycle.url = None;
    kill_and_reap(child);

    let port = *lifecycle.port.get_or_insert_with(free_port);
    lifecycle.generation += 1;
    lifecycle.started_at = Some(now_ms());
    let child = spawn_sidecar(env, port, lifecycle.generation)?;
    let url = format!("http://127.0.0.1:{port}");
    lifecycle.child = Some(child);
    lifecycle.url = Some(url.clone());
    Ok(Some(url))
}

/// Start the bundled OpenCode (idempotent). Returns its base URL. `async`:
/// skill-pack deployment + process spawn at startup must not block the UI
/// thread while the first window paints.
pub fn start_runtime(env: &Env) -> Result<String, String> {
    let mut lifecycle = env.runtime().lifecycle.lock().unwrap();
    if let (Some(_), Some(url)) = (&lifecycle.child, &lifecycle.url) {
        return Ok(url.clone());
    }
    // Repair any impossible partial state left by an older build or a failed
    // transition before attempting a fresh start.
    if let Some(child) = lifecycle.child.take() {
        kill_and_reap(child);
    }
    lifecycle.url = None;

    // Reuse a stable port across restarts so the frontend URL doesn't change.
    let port = *lifecycle.port.get_or_insert_with(free_port);
    lifecycle.generation += 1;
    lifecycle.started_at = Some(now_ms());
    let child = spawn_sidecar(env, port, lifecycle.generation)?;
    let url = format!("http://127.0.0.1:{port}");
    lifecycle.child = Some(child);
    lifecycle.url = Some(url.clone());
    Ok(url)
}

/// Force a fresh sidecar, whatever state the current one is in.
///
/// `start_runtime` reuses a runtime it believes is running, which is right
/// until the process is alive but no longer *serving*: nothing terminates, so
/// nothing clears the lifecycle, and the app retries a port that will never
/// answer again. A dead process is handled by the exit watcher; this is the
/// escape hatch for a wedged one, and the reconnect loop reaches for it once
/// plain retrying has clearly stopped helping.
///
/// Takes a NEW port rather than reusing the old one: the process being killed
/// is by definition not responding, and kill() does not guarantee the port is
/// released by the time we rebind. The caller adopts the returned URL.
pub fn restart_runtime(env: &Env) -> Result<String, String> {
    let mut lifecycle = env.runtime().lifecycle.lock().unwrap();
    if let Some(child) = lifecycle.child.take() {
        kill_and_reap(child);
    }
    lifecycle.url = None;
    lifecycle.port = None;
    let port = *lifecycle.port.get_or_insert_with(free_port);
    lifecycle.generation += 1;
    lifecycle.started_at = Some(now_ms());
    let child = spawn_sidecar(env, port, lifecycle.generation)?;
    let url = format!("http://127.0.0.1:{port}");
    lifecycle.child = Some(child);
    lifecycle.url = Some(url.clone());
    crate::debug_log::append(env, &format!("runtime force-restarted on {url}"));
    Ok(url)
}

/// Epoch ms, for stamping the sidecar's start time.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Epoch ms the current sidecar started, or 0 when none is running. The
/// frontend compares stored message timestamps against it: anything written
/// before this process began cannot be something this process is still
/// producing.
pub fn runtime_started_at(state: &RuntimeState) -> Result<u64, String> {
    let lifecycle = state.lifecycle.lock().map_err(|e| e.to_string())?;
    Ok(lifecycle.started_at.unwrap_or(0))
}

/// The workspace directory the sidecar runs in — the frontend passes it to the
/// SDK so skill discovery is scoped to the right OpenCode instance.
pub fn workspace_path(env: &Env) -> Result<String, String> {
    Ok(workspace_dir(env)?.to_string_lossy().to_string())
}

/// The base folder containing projects and sessions (`~/Documents/HappyScience`).
pub fn workspace_base(env: &Env) -> Result<String, String> {
    Ok(base_workspace_dir(env)?.to_string_lossy().to_string())
}

/// Choose the base folder (Settings → Workspace → Change). Creates its
/// `projects/` and `sessions/` collections and persists the choice. Existing
/// workspaces keep their folders.
pub fn set_workspace_base(env: &Env, path: String) -> Result<String, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_absolute() {
        return Err("workspace base must be absolute".into());
    }
    ensure_base_layout(dir.clone()).map_err(|e| format!("could not create folder: {e}"))?;
    let canon = crate::artifact_file::native_path(&dir.canonicalize().map_err(|e| e.to_string())?);
    std::fs::write(base_workspace_file(env)?, canon.as_bytes()).map_err(|e| e.to_string())?;
    Ok(canon)
}

/// Switch the active workspace folder: create it if needed and persist the
/// choice. The kernel / Files / provenance read the folder via `workspace_dir`;
/// the agent runtime is scoped per request — the frontend reconnects its event
/// stream with `?directory=` and creates sessions with it (a bare `/event`
/// stream would not see other folders' instances, so the scoped stream is
/// required). `path` must be absolute.
pub fn set_workspace(env: &Env, path: String) -> Result<String, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_absolute() {
        return Err("workspace path must be absolute".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create folder: {e}"))?;
    let canon = dir.canonicalize().map_err(|e| e.to_string())?;
    // Persisted and returned in native form — on Windows the verbatim `\\?\`
    // path `canonicalize()` produces matches nothing the sidecar reports (#76).
    let native = crate::artifact_file::native_path(&canon);
    std::fs::write(active_workspace_file(env)?, native.as_bytes()).map_err(|e| e.to_string())?;

    // Follow the active folder with the snapshot watcher so out-of-app edits
    // (external editor, detached process) in the new workspace are captured too.
    crate::git_snapshot::watch_workspace(&canon);

    // No sidecar restart: OpenCode serves every folder from one process via
    // per-directory instances, and the frontend reconnects its event stream
    // with `?directory=<new folder>`. Restarting here used to cost 3-6 s per
    // history-session switch (process boot + reconnect polling).
    Ok(native)
}

/// Record which session owns the active workspace, so bundled skill helpers
/// (record_run.py) can stamp remote runs with their `sessionId` — the app knows
/// the id but the off-app helper only sees the workspace. Written as
/// `<workspace>/.openscience/session.txt`; best-effort, empty ids are ignored.
pub fn mark_session(env: &Env, session_id: String) -> Result<(), String> {
    let id = session_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let dir = workspace_dir(env)?.join(".openscience");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("session.txt");
    // Write-then-rename so a concurrent read never sees a half-written id.
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, id).map_err(|e| e.to_string())?;
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::write(&path, id);
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(())
}

/// Create a new dated folder `<base>/sessions/<name>` and switch to it. `name`
/// is a single path segment (the frontend supplies a timestamp); rejects
/// separators.
pub fn new_dated_workspace(env: &Env, name: String) -> Result<String, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("invalid folder name".into());
    }
    let dir = sessions_dir(env)?.join(&name);
    let canon = set_workspace(env, dir.to_string_lossy().to_string())?;
    // Seed the agent harness into the fresh folder so it starts with its
    // operating rules, not an empty directory. Only NEW dated folders get seeded
    // (never `set_workspace` alone — switching to an existing session must not
    // re-plant the scaffold).
    crate::harness::seed_harness(env, std::path::Path::new(&canon));
    crate::git_snapshot::commit_best_effort(std::path::Path::new(&canon), "Initialize workspace");
    Ok(canon)
}

/// Characters a file name cannot carry on Windows/macOS/Linux, plus control
/// characters. Conversation titles are free text, so a title becomes a file
/// name only after this.
fn safe_file_stem(title: &str, fallback: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    // Windows also rejects a trailing dot or space.
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    // 80 chars leaves room for the id suffix and the extension inside the
    // 255-byte limit every mainstream filesystem enforces.
    let capped: String = trimmed.chars().take(80).collect();
    let capped = capped.trim().to_string();
    if capped.is_empty() {
        return fallback.to_string();
    }
    // Windows refuses these device names whatever the extension.
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| capped.eq_ignore_ascii_case(r)) {
        return format!("{capped}-");
    }
    capped
}

/// Write one exported conversation into a folder the user picked in a native
/// dialog. Confined to that folder: the file name is derived from the title
/// (never used as a path), so a conversation called "../../.ssh/authorized_keys"
/// cannot escape it.
pub fn write_export_file(
    directory: String,
    name: String,
    contents: String,
) -> Result<String, String> {
    let dir = PathBuf::from(&directory);
    if !dir.is_dir() {
        return Err(format!("{directory} is not a folder"));
    }
    let stem = safe_file_stem(&name, "conversation");
    let mut path = dir.join(format!("{stem}.md"));
    // Two conversations can share a title; never silently overwrite one.
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).md"));
        n += 1;
        if n > 999 {
            return Err("too many files with that name".to_string());
        }
    }
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Kill the bundled OpenCode if running.
pub fn stop_runtime(state: &RuntimeState) {
    let mut lifecycle = state.lifecycle.lock().unwrap();
    if let Some(child) = lifecycle.child.take() {
        kill_and_reap(child);
    }
    lifecycle.url = None;
}

pub fn kill_child(state: &RuntimeState) {
    let mut lifecycle = state.lifecycle.lock().unwrap();
    if let Some(child) = lifecycle.child.take() {
        kill_and_reap(child);
    }
    lifecycle.url = None;
}

#[cfg(test)]
mod tests {
    use super::{
        auth_has_provider, base_workspace_dir, dependency_pins, deploy_goal_plugin_dependencies,
        ensure_base_layout, package_dependency_version, parse_scutil_proxy, prune_stale_skills,
        random_hex, remove_key_from_config, resolve_proxy_env, sidecar_filename,
        skill_name_from_markdown, sync_skill_pack, validate_proxy_url, workspace_skill_dirs,
        OPENCODE_PLUGIN_PACKAGE,
    };
    use std::fs;

    #[test]
    fn bundled_sidecars_use_product_scoped_names() {
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        for name in ["opencode", "uv", "agent-browser", "osd"] {
            assert_eq!(
                sidecar_filename(name),
                format!("happy-science-{name}{suffix}")
            );
        }
    }

    /// The rule stated above `quiet_command`, enforced. A raw `Command::new` in
    /// shipped code opens a console window on Windows — 0.4.0 shipped one that
    /// stayed open beside the app for every agent-browser MCP server the runtime
    /// started (#114). Test code is exempt: it never runs inside the packaged app.
    #[test]
    fn shipped_code_never_spawns_with_a_raw_command() {
        const DEFINITION: &str = "let mut cmd = std::process::Command::new(bin);";
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for entry in fs::read_dir(&src).expect("src/ is readable") {
            let path = entry.expect("directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = fs::read_to_string(&path).expect("source is readable");
            // Everything from the file's first `#[cfg(test)]` on is test-only.
            let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (i, line) in shipped.lines().enumerate() {
                if line.contains("Command::new(") && line.trim() != DEFINITION {
                    offenders.push(format!("{}:{}", path.display(), i + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "spawn through crate::runtime::quiet_command instead: {offenders:?}"
        );
    }

    #[test]
    fn sidecar_stderr_splits_on_carriage_returns_and_survives_chunk_boundaries() {
        // The old drain split on BOTH \n and \r; a rewrite that only split on
        // \n would hold a progress line forever (and grow without bound), and
        // decoding each 4 KiB read separately mangles any multi-byte character
        // that straddles the boundary — agent output is routinely non-ASCII.
        let dir = std::env::temp_dir().join(format!("os-drain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let env = crate::env::Env::new(dir.clone(), dir.join("res"), None, "0.0.0".into());

        // A character deliberately straddling the 4096-byte read boundary.
        let mut input = "x".repeat(4095);
        input.push('数');
        // `step 1` is terminated by a BARE carriage return — the case that only
        // a \r-aware split separates from what follows it.
        input.push_str("\nready\nstep 1\rstep 2\ndone\n");
        super::drain_stderr(&env, std::io::Cursor::new(input.into_bytes()));

        let log = fs::read_to_string(dir.join("debug.log")).expect("debug.log written");
        // Each log line is "<ts> [opencode] <text>"; compare the text itself, so
        // a run-together line cannot pass by containing both substrings.
        let lines: Vec<String> = log
            .lines()
            .filter_map(|l| l.split_once("[opencode] ").map(|(_, t)| t.to_string()))
            .collect();
        assert!(lines.iter().any(|l| l == "ready"), "{lines:?}");
        assert!(
            lines.iter().any(|l| l == "step 1"),
            "a \\r must end a line: {lines:?}"
        );
        assert!(lines.iter().any(|l| l == "step 2"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "done"), "{lines:?}");
        assert!(
            !log.contains('\u{fffd}'),
            "a split character was mangled:\n{log}"
        );
        // The straddling character survived intact, on its own line.
        assert!(lines.iter().any(|l| l.ends_with('数')), "{lines:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A killed sidecar must be REAPED, not left as a zombie. Unix-only: this is
    /// where the leak exists, and `ps` is how it is visible.
    #[cfg(unix)]
    #[test]
    fn a_killed_sidecar_leaves_no_zombie() {
        let child = super::quiet_command("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a test child");
        let pid = child.id();
        super::kill_and_reap(child);

        // A zombie still answers `ps` with state Z; a reaped child answers
        // nothing at all. Retry briefly: the kill is asynchronous.
        let mut state = String::new();
        for _ in 0..50 {
            let out = super::quiet_command("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .expect("ps runs");
            state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if state.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            state.is_empty(),
            "pid {pid} is still in the process table as {state:?} — it was killed but never waited on"
        );
    }

    #[test]
    fn auth_store_provider_lookup() {
        let auth = r#"{ "openai": { "type": "oauth", "refresh": "r", "access": "a" } }"#;
        assert!(auth_has_provider(auth, "openai"));
        assert!(!auth_has_provider(auth, "anthropic"));
        assert!(!auth_has_provider("", "openai")); // empty/corrupt store
        assert!(!auth_has_provider("not json", "openai"));
    }

    #[test]
    fn base_layout_has_separate_project_and_session_collections() {
        let root = std::env::temp_dir().join(format!("os-workspace-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        assert_eq!(ensure_base_layout(root.clone()).unwrap(), root);
        assert!(root.join("projects").is_dir());
        assert!(root.join("sessions").is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn new_installs_use_happy_science_without_moving_existing_workspaces() {
        let root =
            std::env::temp_dir().join(format!("happy-science-workspace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let docs = root.join("Documents");
        let legacy = docs.join("OpenScience");
        fs::create_dir_all(&legacy).unwrap();
        let env = crate::env::Env::new(
            root.join("data"),
            root.join("resources"),
            Some(docs.clone()),
            "0.0.0".into(),
        );

        assert_eq!(base_workspace_dir(&env).unwrap(), legacy);
        assert!(!docs.join("HappyScience").exists());

        fs::remove_dir_all(&root).unwrap();
        fs::create_dir_all(&docs).unwrap();
        assert_eq!(base_workspace_dir(&env).unwrap(), docs.join("HappyScience"));
        assert!(docs.join("HappyScience/projects").is_dir());
        assert!(docs.join("HappyScience/sessions").is_dir());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn proxy_url_validation() {
        assert!(validate_proxy_url("http://127.0.0.1:7890").is_ok());
        assert!(validate_proxy_url("socks5://10.0.0.2:1080").is_ok());
        assert!(validate_proxy_url("http://[::1]:8080").is_ok());
        assert!(validate_proxy_url("127.0.0.1:7890").is_err()); // no scheme
        assert!(validate_proxy_url("http://host").is_err()); // no port
        assert!(validate_proxy_url("http://:7890").is_err()); // no host
        assert!(validate_proxy_url("ftp://h:1").is_err()); // wrong scheme
    }

    #[test]
    fn proxy_env_modes() {
        let none = resolve_proxy_env("none", "");
        assert!(none.iter().any(|(k, v)| *k == "NO_PROXY" && v == "*"));
        assert!(none
            .iter()
            .any(|(k, v)| *k == "HTTPS_PROXY" && v.is_empty()));

        let custom = resolve_proxy_env("custom", "http://127.0.0.1:7890");
        assert!(custom
            .iter()
            .any(|(k, v)| *k == "HTTPS_PROXY" && v == "http://127.0.0.1:7890"));
        assert!(custom
            .iter()
            .any(|(k, v)| *k == "NO_PROXY" && v.contains("127.0.0.1")));
    }

    #[test]
    fn scutil_proxy_parses_and_prefers_https() {
        // Real `scutil --proxy` shape (indented `Key : value` lines).
        let all = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 1087\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 1087\n  HTTPSProxy : 127.0.0.1\n  SOCKSEnable : 1\n  SOCKSPort : 1087\n  SOCKSProxy : 127.0.0.1\n}";
        assert_eq!(
            parse_scutil_proxy(all).as_deref(),
            Some("http://127.0.0.1:1087")
        );
        let socks_only = "  SOCKSEnable : 1\n  SOCKSPort : 7890\n  SOCKSProxy : 10.0.0.2\n";
        assert_eq!(
            parse_scutil_proxy(socks_only).as_deref(),
            Some("socks5://10.0.0.2:7890")
        );
        let disabled = "  HTTPEnable : 0\n  HTTPPort : 1087\n  HTTPProxy : 127.0.0.1\n";
        assert_eq!(parse_scutil_proxy(disabled), None);
        assert_eq!(parse_scutil_proxy(""), None);
    }

    #[test]
    fn prune_removes_only_stale_skill_dirs() {
        let dst = std::env::temp_dir().join(format!("os-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dst);
        for name in ["remote-compute", "hpc-slurm"] {
            fs::create_dir_all(dst.join(name)).unwrap();
            fs::write(dst.join(name).join("SKILL.md"), b"---\n").unwrap();
        }
        // A directory without a SKILL.md must never be touched.
        fs::create_dir_all(dst.join("notes")).unwrap();

        let mut bundled = std::collections::HashSet::new();
        bundled.insert(std::ffi::OsString::from("remote-compute"));
        prune_stale_skills(&dst, &bundled);

        assert!(dst.join("remote-compute").is_dir(), "bundled skill kept");
        assert!(
            !dst.join("hpc-slurm").exists(),
            "stale renamed skill removed"
        );
        assert!(dst.join("notes").is_dir(), "non-skill dir left alone");
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn prune_keeps_installed_user_skills() {
        // The reserved `user/` tree holds what the user installed — an app
        // upgrade (which prunes everything unbundled) must never delete it (#61).
        let dst = std::env::temp_dir().join(format!("os-prune-user-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dst);
        let installed = dst.join("user").join("my-skill");
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("SKILL.md"), b"---\nname: my-skill\n---\n").unwrap();
        // A SKILL.md directly inside `user/` (not a skill dir of its own) too.
        fs::write(dst.join("user").join("SKILL.md"), b"---\n").unwrap();

        prune_stale_skills(&dst, &std::collections::HashSet::new());

        assert!(installed.join("SKILL.md").is_file(), "installed skill kept");
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn skill_name_comes_from_frontmatter_and_is_safe() {
        assert_eq!(
            skill_name_from_markdown("---\nname: my-skill\ndescription: x\n---\n\nbody\n")
                .as_deref(),
            Some("my-skill"),
        );
        // Quoted, CRLF, and a leading BOM all still parse.
        assert_eq!(
            skill_name_from_markdown("\u{feff}---\r\nname: \"quoted_1\"\r\n---\r\n").as_deref(),
            Some("quoted_1"),
        );
        // Not a skill file, or a name that cannot be a directory.
        assert_eq!(skill_name_from_markdown("# just markdown\n"), None);
        assert_eq!(skill_name_from_markdown("---\ndescription: x\n---\n"), None);
        assert_eq!(
            skill_name_from_markdown("---\nname: ../escape\n---\n"),
            None
        );
        assert_eq!(skill_name_from_markdown("---\nname: sub/dir\n---\n"), None);
        assert_eq!(skill_name_from_markdown("---\nname: .hidden\n---\n"), None);
        assert_eq!(skill_name_from_markdown("---\nname:\n---\n"), None);
    }

    #[test]
    fn workspace_skill_dirs_lists_only_real_skills() {
        let ws = std::env::temp_dir().join(format!("os-ws-skills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&ws);
        let root = ws.join(".opencode").join("skills");
        fs::create_dir_all(root.join("installed")).unwrap();
        fs::write(root.join("installed").join("SKILL.md"), b"---\n").unwrap();
        fs::create_dir_all(root.join("half-written")).unwrap(); // no SKILL.md yet
        fs::write(root.join("loose.md"), b"---\n").unwrap();

        let found = workspace_skill_dirs(&ws);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "installed");
        // A workspace with no .opencode/skills at all is simply empty.
        assert!(workspace_skill_dirs(&std::env::temp_dir().join("os-nope")).is_empty());
        let _ = fs::remove_dir_all(&ws);
    }

    #[cfg(unix)]
    #[test]
    fn tighten_private_makes_dir_and_secrets_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("os-private-{}", std::process::id()));
        let sub = dir.join("opencode");
        fs::create_dir_all(&sub).unwrap();
        let cfg = sub.join("opencode.jsonc");
        fs::write(&cfg, b"{\"apiKey\":\"secret\"}").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&cfg, fs::Permissions::from_mode(0o644)).unwrap();

        // The runtime root holds provider/connector keys (opencode.jsonc,
        // auth.json) — it must be unreadable to other users even when the
        // sidecar later rewrites files inside with a default umask.
        super::tighten_private(&dir);
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        super::tighten_private(&cfg);
        assert_eq!(
            fs::metadata(&cfg).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn random_hex_is_csprng_shaped() {
        // 16 bytes → 32 hex chars, fresh per call — the shape the sidecar
        // password and the preview/Jupyter tokens rely on.
        let a = random_hex(16);
        let b = random_hex(16);
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "two draws must differ");
    }

    #[test]
    fn removes_only_the_named_config_entry() {
        let cfg = r#"{"model":"a/b","provider":{"ollama":{"npm":"x"},"keep":{"npm":"y"}},"mcp":{"pw":{"type":"local"}}}"#;
        let out = remove_key_from_config(cfg, "provider", "ollama").unwrap();
        assert!(!out.contains("ollama"));
        assert!(out.contains("keep"));
        assert!(out.contains("\"model\": \"a/b\""));
        let out2 = remove_key_from_config(cfg, "mcp", "pw").unwrap();
        assert!(!out2.contains("\"pw\""));
        // Absent key and non-JSON input are errors, not silent no-ops.
        assert!(remove_key_from_config(cfg, "provider", "missing").is_err());
        assert!(remove_key_from_config("// jsonc comment\n{}", "provider", "x").is_err());
    }

    fn write(path: &std::path::Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// The hazard that shapes `spawn_tied_to_our_lifetime`, modelled directly.
    ///
    /// PR_SET_PDEATHSIG binds to the THREAD that created the child, and the
    /// desktop app starts the runtime from a Tauri command — a pool thread that
    /// exits right after. Spawning there would have the kernel kill the sidecar
    /// moments later, turning a leak fix into a far worse bug. `osd server`
    /// spawns from its main thread and would never have shown it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_child_spawned_from_a_short_lived_thread_outlives_that_thread() {
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30");
        let spawned = std::thread::spawn(move || {
            super::spawn_tied_to_our_lifetime(cmd).expect("the child spawns")
        });
        // The thread is gone from here on, exactly like the command's.
        let mut child = spawned.join().expect("the spawning thread finished");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let alive = child.try_wait().expect("try_wait").is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            alive,
            "the child was killed when its spawning thread exited — PDEATHSIG must be bound to a \
             thread that lives as long as the process"
        );
    }

    #[test]
    fn deploys_goal_plugin_dependencies_without_network() {
        let tmp = std::env::temp_dir().join(format!("goal-deps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        write(&src.join(".opencode-plugin-version"), "1.17.13\n");
        write(
            &src.join("package.json"),
            r#"{"dependencies":{"@opencode-ai/plugin":"1.17.13"}}"#,
        );
        write(&src.join("package-lock.json"), "{}");
        write(
            &src.join("node_modules/@opencode-ai/plugin/package.json"),
            r#"{"name":"@opencode-ai/plugin","version":"1.17.13"}"#,
        );
        write(
            &src.join("node_modules/@opencode-ai/plugin/dist/tool.js"),
            "export const tool = (x) => x;",
        );

        deploy_goal_plugin_dependencies(&src, &dst).unwrap();

        assert_eq!(
            fs::read_to_string(dst.join(".opencode-plugin-version")).unwrap(),
            "1.17.13\n"
        );
        assert!(dst
            .join("node_modules/@opencode-ai/plugin/dist/tool.js")
            .is_file());
        assert!(dst.join("package-lock.json").is_file());
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn deploys_the_caret_range_npm_actually_writes() {
        // Regression: every test above pinned the dependency exactly, but
        // `npm install @opencode-ai/plugin@1.18.18` — which is what
        // fetch-goal-plugin.sh runs — writes `^1.18.18`. Comparing that range
        // to the exact bundled version as strings never matched, so the
        // post-copy check failed on EVERY fresh profile: no goal plugin was
        // deployed, none was registered, and `/goal` silently did nothing.
        // Found by unpacking a release archive and starting a server from it.
        let tmp = std::env::temp_dir().join(format!("goal-deps-caret-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        write(&src.join(".opencode-plugin-version"), "1.18.18\n");
        write(
            &src.join("package.json"),
            r#"{"dependencies":{"@opencode-ai/plugin":"^1.18.18"}}"#,
        );
        write(&src.join("package-lock.json"), "{}");
        write(
            &src.join("node_modules/@opencode-ai/plugin/package.json"),
            r#"{"name":"@opencode-ai/plugin","version":"1.18.18"}"#,
        );

        deploy_goal_plugin_dependencies(&src, &dst).expect("a caret range still pins 1.18.18");
        assert!(dst
            .join("node_modules/@opencode-ai/plugin/package.json")
            .is_file());

        // A range that pins a DIFFERENT version is still a real mismatch.
        assert!(dependency_pins("^1.18.18", "1.18.18"));
        assert!(dependency_pins("~1.18.18", "1.18.18"));
        assert!(dependency_pins("1.18.18", "1.18.18"));
        assert!(!dependency_pins("^1.17.13", "1.18.18"));
        assert!(!dependency_pins("*", "1.18.18"));
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn adopts_existing_goal_plugin_dependencies_without_recopying() {
        let tmp = std::env::temp_dir().join(format!("goal-deps-existing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        write(&src.join(".opencode-plugin-version"), "1.17.13\n");
        write(
            &dst.join("package.json"),
            r#"{"dependencies":{"@opencode-ai/plugin":"1.17.13","user-plugin":"2.0.0"}}"#,
        );
        write(&dst.join("package-lock.json"), "{}");
        write(
            &dst.join("node_modules/@opencode-ai/plugin/package.json"),
            r#"{"name":"@opencode-ai/plugin","version":"1.17.13"}"#,
        );
        write(&dst.join("node_modules/user-plugin/keep.txt"), "keep");

        deploy_goal_plugin_dependencies(&src, &dst).unwrap();

        assert_eq!(
            fs::read_to_string(dst.join(".opencode-plugin-version")).unwrap(),
            "1.17.13\n"
        );
        assert_eq!(
            fs::read_to_string(dst.join("node_modules/user-plugin/keep.txt")).unwrap(),
            "keep"
        );
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn an_old_profiles_stale_pin_is_brought_up_to_the_bundled_version() {
        // The shape of a real profile created in July, found by starting 0.5.0
        // against it: package.json and the lockfile still name 1.17.13 (the
        // version the app that created the profile bundled), while node_modules
        // already holds 1.18.18. package.json was only ever copied when absent,
        // so the stale line survived every upgrade, the post-check failed on
        // every start, and `deploy_goal_plugin` bailed before its refresh —
        // the profile was stuck with the plugin build it received first.
        let tmp = std::env::temp_dir().join(format!("goal-deps-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        write(&src.join(".opencode-plugin-version"), "1.18.18\n");
        write(
            &src.join("package.json"),
            r#"{"dependencies":{"@opencode-ai/plugin":"^1.18.18"}}"#,
        );
        write(
            &src.join("package-lock.json"),
            r#"{"lockfileVersion":3,"packages":{"":{"dependencies":{"@opencode-ai/plugin":"^1.18.18"}}}}"#,
        );
        write(
            &src.join("node_modules/@opencode-ai/plugin/package.json"),
            r#"{"name":"@opencode-ai/plugin","version":"1.18.18"}"#,
        );
        write(
            &dst.join("package.json"),
            "{\n  \"dependencies\": {\n    \"@opencode-ai/plugin\": \"1.17.13\"\n  }\n}\n",
        );
        write(
            &dst.join("package-lock.json"),
            r#"{"lockfileVersion":3,"packages":{"":{"dependencies":{"@opencode-ai/plugin":"1.17.13"}}}}"#,
        );

        deploy_goal_plugin_dependencies(&src, &dst)
            .expect("a stale pin must be adopted, not fatal");

        assert_eq!(
            package_dependency_version(&dst.join("package.json"), OPENCODE_PLUGIN_PACKAGE)
                .as_deref(),
            Some("^1.18.18"),
            "the app's own dependency line must follow the runtime it bundles"
        );
        // The lockfile named the old version too: left behind, the next
        // `npm install` would resolve 1.17.13 back over the tree just copied.
        assert!(
            fs::read_to_string(dst.join("package-lock.json"))
                .unwrap()
                .contains("1.18.18"),
            "a lockfile holding nothing but our dependency is refreshed"
        );
        assert_eq!(
            fs::read_to_string(dst.join(".opencode-plugin-version")).unwrap(),
            "1.18.18\n"
        );
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_stale_pin_is_updated_without_disturbing_the_users_own_dependencies() {
        let tmp = std::env::temp_dir().join(format!("goal-deps-mixed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        write(&src.join(".opencode-plugin-version"), "1.18.18\n");
        write(
            &src.join("package.json"),
            r#"{"dependencies":{"@opencode-ai/plugin":"^1.18.18"}}"#,
        );
        write(&src.join("package-lock.json"), r#"{"lockfileVersion":3}"#);
        write(
            &src.join("node_modules/@opencode-ai/plugin/package.json"),
            r#"{"name":"@opencode-ai/plugin","version":"1.18.18"}"#,
        );
        write(
            &dst.join("package.json"),
            r#"{"name":"opencode","dependencies":{"@opencode-ai/plugin":"1.17.13","user-plugin":"2.0.0"}}"#,
        );
        write(
            &dst.join("package-lock.json"),
            r#"{"lockfileVersion":3,"packages":{"":{"dependencies":{"@opencode-ai/plugin":"1.17.13","user-plugin":"2.0.0"}}}}"#,
        );

        deploy_goal_plugin_dependencies(&src, &dst).unwrap();

        let text = fs::read_to_string(dst.join("package.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(doc["dependencies"]["@opencode-ai/plugin"], "^1.18.18");
        assert_eq!(
            doc["dependencies"]["user-plugin"], "2.0.0",
            "the user's own plugin dependency must survive"
        );
        assert_eq!(doc["name"], "opencode", "every other key is written back");
        // Their lockfile resolves their package too, so it is not ours to replace.
        assert!(
            fs::read_to_string(dst.join("package-lock.json"))
                .unwrap()
                .contains("user-plugin"),
            "a lockfile holding the user's packages is left alone"
        );
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn an_unreadable_profile_package_json_is_reported_not_replaced() {
        // #116's lesson: a config the app cannot parse must never be destroyed.
        let tmp = std::env::temp_dir().join(format!("goal-deps-broken-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        write(&src.join(".opencode-plugin-version"), "1.18.18\n");
        write(
            &src.join("package.json"),
            r#"{"dependencies":{"@opencode-ai/plugin":"^1.18.18"}}"#,
        );
        write(&src.join("package-lock.json"), r#"{"lockfileVersion":3}"#);
        write(
            &src.join("node_modules/@opencode-ai/plugin/package.json"),
            r#"{"name":"@opencode-ai/plugin","version":"1.18.18"}"#,
        );
        let broken = "{ this is not json";
        write(&dst.join("package.json"), broken);

        let err = deploy_goal_plugin_dependencies(&src, &dst)
            .expect_err("an unparseable package.json must not be adopted silently");
        assert!(err.contains("not readable JSON"), "{err}");
        assert_eq!(
            fs::read_to_string(dst.join("package.json")).unwrap(),
            broken,
            "the file the app cannot read is left byte-for-byte"
        );
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn sync_replaces_bundled_and_keeps_user_skills() {
        let tmp = std::env::temp_dir().join(format!("skillsync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        let dst = tmp.join("dst");

        // Bundled pack: one skill with a nested reference file, plus a top-level
        // plain file (.commit) that must NOT be copied.
        write(&src.join("paper-writer/SKILL.md"), "v2");
        write(&src.join("paper-writer/references/guide.md"), "ref");
        write(&src.join(".commit"), "abc123");
        // A placeholder dir without SKILL.md must not be deployed.
        fs::create_dir_all(src.join("placeholder")).unwrap();

        // Existing workspace: a stale copy of the bundled skill (with a file the
        // new version no longer has) and a user-installed skill.
        write(&dst.join("paper-writer/SKILL.md"), "v1");
        write(&dst.join("paper-writer/obsolete.md"), "old");
        write(&dst.join("my-skill/SKILL.md"), "user");

        sync_skill_pack(&src, &dst).unwrap();

        assert_eq!(
            fs::read_to_string(dst.join("paper-writer/SKILL.md")).unwrap(),
            "v2"
        );
        assert_eq!(
            fs::read_to_string(dst.join("paper-writer/references/guide.md")).unwrap(),
            "ref"
        );
        assert!(
            !dst.join("paper-writer/obsolete.md").exists(),
            "stale file must be gone"
        );
        assert_eq!(
            fs::read_to_string(dst.join("my-skill/SKILL.md")).unwrap(),
            "user"
        );
        assert!(
            !dst.join(".commit").exists(),
            "top-level files are not skills"
        );
        assert!(
            !dst.join("placeholder").exists(),
            "dirs without SKILL.md are not skills"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn sync_creates_destination_when_missing() {
        let tmp = std::env::temp_dir().join(format!("skillsync-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let src = tmp.join("src");
        write(&src.join("literature-survey/SKILL.md"), "s");

        let dst = tmp.join("deep/nested/skills");
        sync_skill_pack(&src, &dst).unwrap();
        assert_eq!(
            fs::read_to_string(dst.join("literature-survey/SKILL.md")).unwrap(),
            "s"
        );
        fs::remove_dir_all(&tmp).unwrap();
    }
}

/// Remove an entry from a map section of the app-private global OpenCode
/// config ("provider" or "mcp") and restart the sidecar (PATCH /global/config
/// cannot delete keys).
pub fn remove_config_entry(env: &Env, section: String, key: String) -> Result<(), String> {
    if !matches!(section.as_str(), "provider" | "mcp") {
        return Err(format!("section \"{section}\" is not removable"));
    }
    let dir = xdg_config_home(env)?.join("opencode");
    // The server writes opencode.jsonc; older configs may be opencode.json.
    let path = ["opencode.jsonc", "opencode.json"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
        .ok_or("no global OpenCode config found")?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let out = remove_key_from_config(&text, &section, &key)?;
    std::fs::write(&path, out).map_err(|e| e.to_string())?;
    tighten_private(&path);

    restart_sidecar_if_running(env)?;
    Ok(())
}

/// Drop `key` from the config JSON's `section` map, erroring when the config
/// is not plain JSON or the key is absent.
fn remove_key_from_config(text: &str, section: &str, key: &str) -> Result<String, String> {
    let mut cfg: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("config is not plain JSON: {e}"))?;
    let removed = cfg
        .get_mut(section)
        .and_then(|p| p.as_object_mut())
        .map(|p| p.remove(key).is_some())
        .unwrap_or(false);
    if !removed {
        return Err(format!(
            "\"{key}\" is not in the config's {section} section"
        ));
    }
    serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())
}

/// The current approval mode ("approve" | "full"). Spawn seeding guarantees a
/// mode exists once the runtime has started; before that, report the default.
/// The default model this machine's runtime is configured with. Read from the
/// config rather than a gateway, so it answers before any server is up — which
/// is when a fresh headless box gets configured.
pub fn get_default_model(env: &Env) -> Result<Option<String>, String> {
    let existing = std::fs::read_to_string(effective_config_file(env)?).unwrap_or_default();
    Ok(crate::opencode_config::default_model_of(&existing))
}

/// Set the default model on this machine and restart the sidecar if one is
/// running, so the next turn uses it.
pub fn set_default_model(env: &Env, model: String) -> Result<(), String> {
    let path = effective_config_file(env)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated =
        crate::opencode_config::set_default_model(&existing, &model).ok_or_else(|| {
            format!(
                "{} could not be parsed, so it was left untouched — fix the JSON and try again",
                path.display()
            )
        })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    tighten_private(&path);
    restart_sidecar_if_running(env)?;
    Ok(())
}

pub fn get_approval_mode(env: &Env) -> Result<String, String> {
    let path = effective_config_file(env)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    Ok(crate::opencode_config::permission_mode_of(&existing)
        .unwrap_or(crate::opencode_config::MODE_APPROVE)
        .to_string())
}

/// Switch the approval mode and restart the sidecar so the permission rules
/// take effect. Returns the (stable-port) base URL when it was running.
pub fn set_approval_mode(env: &Env, mode: String) -> Result<String, String> {
    let path = effective_config_file(env)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = crate::opencode_config::set_permission_mode(&existing, &mode)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    tighten_private(&path);

    // Same restart flow as configure_opencode: reload rules on a stable port.
    Ok(restart_sidecar_if_running(env)?.unwrap_or_else(|| path.to_string_lossy().to_string()))
}

/// The global memory file: one Markdown document that OpenCode loads into
/// every conversation, in the app-private profile next to the config.
fn global_memory_file(env: &Env) -> Result<PathBuf, String> {
    Ok(xdg_config_home(env)?.join("opencode").join("MEMORY.md"))
}

/// Absolute path of a memory file, forward-slashed so the config stays
/// portable. `scope` is "global" (the profile file) or "project" (that
/// folder's own AGENTS.md — the file OpenCode loads for sessions inside it).
fn memory_file(env: &Env, scope: &str, directory: Option<&str>) -> Result<PathBuf, String> {
    match scope {
        "global" => global_memory_file(env),
        "project" => {
            let dir = directory
                .filter(|d| !d.is_empty())
                .ok_or("no project folder")?;
            Ok(PathBuf::from(dir).join(crate::opencode_config::PROJECT_MEMORY_FILE))
        }
        other => Err(format!("unknown memory scope \"{other}\"")),
    }
}

/// Read a memory layer. A file that was never written reads as empty — the
/// editor opens blank rather than erroring.
pub fn read_memory(env: &Env, scope: String, directory: Option<String>) -> Result<String, String> {
    let path = memory_file(env, &scope, directory.as_deref())?;
    Ok(std::fs::read_to_string(path).unwrap_or_default())
}

/// Replace a memory layer's contents. Writing an empty document deletes the
/// file, so "cleared" and "never set" stay the same state.
pub fn write_memory(
    env: &Env,
    scope: String,
    directory: Option<String>,
    text: String,
) -> Result<(), String> {
    let path = memory_file(env, &scope, directory.as_deref())?;
    if text.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

/// Append a block to a memory layer, keeping what is already there. This is
/// what "save this to memory" from a conversation does.
pub fn append_memory(
    env: &Env,
    scope: String,
    directory: Option<String>,
    text: String,
) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let path = memory_file(env, &scope, directory.as_deref())?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut out = existing.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(trimmed);
    out.push('\n');
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

/// Whether the memory layers are currently applied to conversations.
pub fn get_memory_enabled(env: &Env) -> Result<bool, String> {
    let global = global_memory_file(env)?
        .to_string_lossy()
        .replace('\\', "/");
    let existing = std::fs::read_to_string(effective_config_file(env)?).unwrap_or_default();
    Ok(crate::opencode_config::memory_enabled(&existing, &global))
}

/// Apply or stop applying the memory layers, restarting the sidecar so the
/// change takes effect (instructions are read when a session's context is built).
pub fn set_memory_enabled(env: &Env, enabled: bool) -> Result<(), String> {
    let global = global_memory_file(env)?
        .to_string_lossy()
        .replace('\\', "/");
    let path = effective_config_file(env)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Some(updated) = crate::opencode_config::set_memory_enabled(&existing, &global, enabled)
    else {
        return Ok(()); // already in the requested state — no restart
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    tighten_private(&path);
    restart_sidecar_if_running(env)?;
    Ok(())
}

/// Per-agent model overrides as `{ agent: "provider/model" }`.
pub fn get_agent_models(env: &Env) -> Result<serde_json::Value, String> {
    let existing = std::fs::read_to_string(effective_config_file(env)?).unwrap_or_default();
    let map: serde_json::Map<String, serde_json::Value> =
        crate::opencode_config::agent_models(&existing)
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
    Ok(serde_json::Value::Object(map))
}

/// Per-agent reasoning-effort overrides as `{ agent: "high" }` (#71).
pub fn get_agent_variants(env: &Env) -> Result<serde_json::Value, String> {
    let existing = std::fs::read_to_string(effective_config_file(env)?).unwrap_or_default();
    let map: serde_json::Map<String, serde_json::Value> =
        crate::opencode_config::agent_variants(&existing)
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
    Ok(serde_json::Value::Object(map))
}

/// Persist one rewritten config and restart the sidecar, unless the rewrite is a
/// no-op. Shared by the per-agent model and effort writers.
fn write_agent_config(env: &Env, rewrite: impl FnOnce(&str) -> String) -> Result<(), String> {
    let path = effective_config_file(env)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = rewrite(&existing);
    if updated == existing {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    tighten_private(&path);
    restart_sidecar_if_running(env)?;
    Ok(())
}

/// Pin one agent to its own model, or clear the override with an empty model.
/// Restarts the sidecar: agent definitions are built when it loads its config.
pub fn set_agent_model(env: &Env, agent: String, model: String) -> Result<(), String> {
    write_agent_config(env, |existing| {
        let want = if model.is_empty() {
            None
        } else {
            Some(model.as_str())
        };
        crate::opencode_config::set_agent_model(existing, &agent, want)
    })
}

/// Pin one agent to a reasoning-effort variant, or clear it with an empty string.
/// Restarts the sidecar for the same reason `set_agent_model` does.
pub fn set_agent_variant(env: &Env, agent: String, variant: String) -> Result<(), String> {
    write_agent_config(env, |existing| {
        let want = if variant.is_empty() {
            None
        } else {
            Some(variant.as_str())
        };
        crate::opencode_config::set_agent_variant(existing, &agent, want)
    })
}

/// The persisted proxy setting plus the proxy the sidecar would use right now.
pub fn get_proxy_setting(env: &Env) -> Result<serde_json::Value, String> {
    let (mode, url) = read_proxy_setting(env);
    let effective = effective_proxy(&mode, &url);
    Ok(serde_json::json!({ "mode": mode, "url": url, "effective": effective }))
}

/// Persist the proxy setting ("system" | "custom" | "none", url for custom)
/// and restart the sidecar so its network env takes effect.
pub fn set_proxy_setting(env: &Env, mode: String, url: String) -> Result<String, String> {
    let line = match mode.as_str() {
        "system" => "system".to_string(),
        "none" => "none".to_string(),
        "custom" => {
            let url = url.trim();
            validate_proxy_url(url)?;
            format!("custom {url}")
        }
        other => return Err(format!("unknown proxy mode: {other}")),
    };
    let path = proxy_setting_file(env)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, line).map_err(|e| e.to_string())?;

    // Same restart flow as set_approval_mode: the env only applies at spawn.
    Ok(restart_sidecar_if_running(env)?.unwrap_or_else(|| path.to_string_lossy().to_string()))
}

/// The persisted uv mirrors (empty string ⇒ use uv's default index/mirror).
pub fn get_mirror_setting(env: &Env) -> Result<serde_json::Value, String> {
    let (pypi, python) = read_mirror_setting(env);
    Ok(serde_json::json!({ "pypi": pypi, "python": python }))
}

/// Persist the uv mirrors. Blank fields clear that mirror. No sidecar restart:
/// only the next provisioning run (Jupyter / science MCP) reads them.
pub fn set_mirror_setting(env: &Env, pypi: String, python: String) -> Result<(), String> {
    let (pypi, python) = (pypi.trim(), python.trim());
    let mut lines = Vec::new();
    if !pypi.is_empty() {
        validate_mirror_url(pypi)?;
        lines.push(format!("pypi {pypi}"));
    }
    if !python.is_empty() {
        validate_mirror_url(python)?;
        lines.push(format!("python {python}"));
    }
    let path = mirror_setting_file(env)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, lines.join("\n")).map_err(|e| e.to_string())
}

/// Write the provider key/model into the app-private OpenCode config and restart
/// the sidecar so it picks them up. Returns the same base URL (stable port).
pub fn configure_opencode(
    env: &Env,
    provider: String,
    api_key: String,
    model: String,
    base_url: Option<String>,
) -> Result<String, String> {
    let path = opencode_config_file(env)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let merged = merge_config(&existing, &provider, &api_key, &model, base_url.as_deref())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, merged).map_err(|e| e.to_string())?;
    tighten_private(&path);

    // Restart so the running server reloads the new provider config.
    Ok(restart_sidecar_if_running(env)?.unwrap_or_else(|| path.to_string_lossy().to_string()))
}

/// Which providers this machine has credentials for: the ones written into the
/// app-private config, plus the ones OpenCode's own credential store holds from
/// a browser login. NAMES ONLY — no key value is ever returned, printed, or
/// logged by anything (AGENTS.md).
pub fn configured_providers(env: &Env) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    let config = std::fs::read_to_string(effective_config_file(env)?).unwrap_or_default();
    if let Some(map) = crate::opencode_config::read_config(&config)
        .as_ref()
        .and_then(|v| v.get("provider"))
        .and_then(|p| p.as_object())
    {
        names.extend(map.keys().cloned());
    }
    let auth = std::fs::read_to_string(xdg_data_home(env)?.join("opencode").join("auth.json"))
        .unwrap_or_default();
    if let Some(map) = serde_json::from_str::<serde_json::Value>(&auth)
        .ok()
        .as_ref()
        .and_then(|v| v.as_object())
    {
        names.extend(map.keys().cloned());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod export_tests {
    use super::safe_file_stem;

    #[test]
    fn a_title_can_never_become_a_path() {
        // Separators become dashes, so the result can only ever be a leaf name.
        assert_eq!(
            safe_file_stem("../../.ssh/authorized_keys", "x"),
            "..-..-.ssh-authorized_keys"
        );
        assert_eq!(
            safe_file_stem("C:\\Windows\\System32", "x"),
            "C--Windows-System32"
        );
    }

    #[test]
    fn keeps_ordinary_titles_readable_including_non_latin() {
        assert_eq!(
            safe_file_stem("Spike sorting — pass 2", "x"),
            "Spike sorting — pass 2"
        );
        assert_eq!(safe_file_stem("脑机接口趋势分析", "x"), "脑机接口趋势分析");
    }

    #[test]
    fn falls_back_when_a_title_leaves_nothing_usable() {
        assert_eq!(safe_file_stem("   ", "conversation"), "conversation");
        // Nothing but separators still yields a harmless leaf name.
        assert_eq!(safe_file_stem("///", "conversation"), "---");
        // Windows rejects a trailing dot.
        assert_eq!(safe_file_stem("results.", "x"), "results");
    }

    #[test]
    fn sidesteps_windows_device_names() {
        assert_eq!(safe_file_stem("CON", "x"), "CON-");
        assert_eq!(safe_file_stem("nul", "x"), "nul-");
        assert_eq!(safe_file_stem("console", "x"), "console");
    }

    #[test]
    fn caps_the_length_so_the_filesystem_accepts_it() {
        let long = "n".repeat(500);
        assert_eq!(safe_file_stem(&long, "x").chars().count(), 80);
    }
}
