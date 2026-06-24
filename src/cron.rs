use anyhow::{anyhow, Result};
use colored::Colorize;
use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MANAGED_BEGIN: &str = "# mihoto: begin auto-update";
const MANAGED_END: &str = "# mihoto: end auto-update";

#[derive(Clone, Debug, Eq, PartialEq)]
struct CrontabSnapshot {
    content: String,
    existed: bool,
}

/// Get the path to the staging file used by `crontab <file>`.
fn crontab_path() -> PathBuf {
    let run_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        // Use current user's UID as fallback
        let uid = fs::metadata(".").map(|m| m.uid()).unwrap_or(1000);
        format!("/run/user/{}", uid)
    });
    PathBuf::from(run_dir).join("mihoro-crontab")
}

/// Get the path for a best-effort pre-write crontab backup.
fn crontab_backup_path() -> PathBuf {
    let base = env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|_| env::var("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|_| env::var("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|_| env::temp_dir());
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    base.join("mihoro").join(format!("crontab-{}.bak", secs))
}

/// Get the mihoro binary path from current executable
fn mihoro_bin_path() -> Result<String> {
    env::current_exe()?
        .to_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("Failed to get mihoro binary path"))
}

fn quote_cron_command(command: &str) -> String {
    format!("'{}'", command.replace('\'', "'\\''"))
}

fn generate_cron_entry_for_bin(interval_hours: u16, bin_path: &str) -> String {
    format!(
        "0 */{} * * * {} update",
        interval_hours,
        quote_cron_command(bin_path)
    )
}

/// Generate cron entry for auto-update
fn generate_cron_entry(interval_hours: u16) -> Result<String> {
    let bin_path = mihoro_bin_path()?;
    Ok(format!(
        "{}\n",
        generate_cron_entry_for_bin(interval_hours, &bin_path)
    ))
}

fn build_managed_cron_block(interval_hours: u16, bin_path: &str) -> String {
    format!(
        "{}\n{}\n{}\n",
        MANAGED_BEGIN,
        generate_cron_entry_for_bin(interval_hours, bin_path),
        MANAGED_END
    )
}

fn remove_marked_cron_blocks(content: &str) -> (String, usize) {
    let mut output = String::with_capacity(content.len());
    let mut pending_block: Option<String> = None;
    let mut removed = 0;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']).trim();

        if let Some(block) = pending_block.as_mut() {
            block.push_str(line);
            if trimmed == MANAGED_END {
                pending_block = None;
                removed += 1;
            }
            continue;
        }

        if trimmed == MANAGED_BEGIN {
            pending_block = Some(line.to_string());
        } else {
            output.push_str(line);
        }
    }

    if let Some(block) = pending_block {
        output.push_str(&block);
    }

    (output, removed)
}

fn count_managed_cron_blocks(content: &str) -> usize {
    let (_, removed) = remove_marked_cron_blocks(content);
    removed
}

fn cron_command_after_schedule(line: &str) -> Option<&str> {
    let mut fields = 0;
    let mut in_field = false;

    for (idx, ch) in line.char_indices() {
        if ch.is_whitespace() {
            if in_field {
                fields += 1;
                in_field = false;
                if fields == 5 {
                    return Some(line[idx..].trim_start());
                }
            }
        } else {
            in_field = true;
        }
    }

    None
}

fn is_legacy_current_binary_entry(line: &str, bin_path: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }

    let Some(command) = cron_command_after_schedule(trimmed) else {
        return false;
    };
    command == format!("{} update", bin_path)
        || command == format!("{} update", quote_cron_command(bin_path))
}

fn remove_legacy_current_binary_entries(content: &str, bin_path: &str) -> String {
    let mut output = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !is_legacy_current_binary_entry(trimmed, bin_path) {
            output.push_str(line);
        }
    }

    output
}

fn remove_managed_cron_content(content: &str, bin_path: &str) -> String {
    let (without_blocks, _) = remove_marked_cron_blocks(content);
    remove_legacy_current_binary_entries(&without_blocks, bin_path)
}

fn merge_managed_cron_block(existing: &str, block: &str, bin_path: &str) -> String {
    let mut merged = remove_managed_cron_content(existing, bin_path);
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(block);
    merged
}

fn is_no_crontab_message(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("no crontab")
}

fn snapshot_from_failed_crontab_list(stderr: &str) -> Result<CrontabSnapshot> {
    if is_no_crontab_message(stderr) {
        return Ok(CrontabSnapshot {
            content: String::new(),
            existed: false,
        });
    }

    anyhow::bail!("Failed to read crontab: {}", stderr.trim());
}

fn read_current_crontab() -> Result<CrontabSnapshot> {
    let output = Command::new("crontab").arg("-l").output()?;
    if output.status.success() {
        return Ok(CrontabSnapshot {
            content: String::from_utf8_lossy(&output.stdout).into_owned(),
            existed: true,
        });
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    snapshot_from_failed_crontab_list(&stderr)
}

fn install_crontab(content: &str) -> Result<()> {
    let crontab_file = crontab_path();
    fs::write(&crontab_file, content)?;
    let status = Command::new("crontab").arg(&crontab_file).status()?;
    if !status.success() {
        anyhow::bail!("Failed to install crontab");
    }
    Ok(())
}

fn remove_crontab() -> Result<()> {
    let output = Command::new("crontab").arg("-r").output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_no_crontab_message(&stderr) {
        return Ok(());
    }

    anyhow::bail!("Failed to remove crontab: {}", stderr.trim());
}

fn write_crontab_content(content: &str) -> Result<()> {
    if content.is_empty() {
        remove_crontab()
    } else {
        install_crontab(content)
    }
}

fn write_crontab_backup(snapshot: &CrontabSnapshot) -> Result<PathBuf> {
    let backup_path = crontab_backup_path();
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&backup_path, &snapshot.content)?;
    Ok(backup_path)
}

fn restore_crontab(snapshot: &CrontabSnapshot) -> Result<()> {
    if snapshot.existed && !snapshot.content.is_empty() {
        install_crontab(&snapshot.content)
    } else {
        remove_crontab()
    }
}

fn verify_auto_update_enabled(expected_block: &str, bin_path: &str) -> Result<()> {
    let snapshot = read_current_crontab()?;
    if count_managed_cron_blocks(&snapshot.content) != 1 {
        anyhow::bail!("Expected exactly one mihoto-managed cron block after enable");
    }
    if !snapshot.content.contains(expected_block) {
        anyhow::bail!("Installed crontab does not contain the expected mihoto cron block");
    }
    if remove_legacy_current_binary_entries(&snapshot.content, bin_path) != snapshot.content {
        anyhow::bail!("Installed crontab still contains a legacy mihoto cron entry");
    }
    Ok(())
}

fn verify_auto_update_disabled(bin_path: &str) -> Result<()> {
    let snapshot = read_current_crontab()?;
    if count_managed_cron_blocks(&snapshot.content) != 0 {
        anyhow::bail!("Expected no mihoto-managed cron block after disable");
    }
    if remove_legacy_current_binary_entries(&snapshot.content, bin_path) != snapshot.content {
        anyhow::bail!("Crontab still contains a legacy mihoto cron entry after disable");
    }
    Ok(())
}

fn find_mihoto_cron_entry(content: &str, bin_path: &str) -> Option<String> {
    let mut inside_block = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == MANAGED_BEGIN {
            inside_block = true;
            continue;
        }
        if trimmed == MANAGED_END {
            inside_block = false;
            continue;
        }
        if inside_block && !trimmed.is_empty() && !trimmed.starts_with('#') {
            return Some(line.to_string());
        }
        if is_legacy_current_binary_entry(line, bin_path) {
            return Some(line.to_string());
        }
    }
    None
}

/// Enable auto-update by installing or refreshing only the mihoto-managed cron block.
pub fn enable_auto_update(interval_hours: u16, prefix: &str) -> Result<()> {
    if interval_hours == 0 {
        println!(
            "{} Auto-update interval is 0, disabling auto-update",
            prefix.yellow()
        );
        return disable_auto_update(prefix);
    }

    if interval_hours > 24 {
        anyhow::bail!("Auto-update interval must be between 1 and 24 hours");
    }

    let bin_path = mihoro_bin_path()?;
    let snapshot = read_current_crontab()?;
    let _backup_path = write_crontab_backup(&snapshot)?;
    let block = build_managed_cron_block(interval_hours, &bin_path);
    let merged = merge_managed_cron_block(&snapshot.content, &block, &bin_path);

    write_crontab_content(&merged)?;
    if let Err(err) = verify_auto_update_enabled(&block, &bin_path) {
        if let Err(rollback_err) = restore_crontab(&snapshot) {
            anyhow::bail!("{}; rollback failed: {}", err, rollback_err);
        }
        return Err(err);
    }

    println!(
        "{} Auto-update enabled with interval: {} hours",
        prefix.green().bold(),
        interval_hours.to_string().yellow()
    );
    println!(
        "{} Cron entry: {}",
        "->".dimmed(),
        generate_cron_entry(interval_hours)?.trim()
    );

    Ok(())
}

/// Disable auto-update by removing only the mihoto-managed cron block.
pub fn disable_auto_update(prefix: &str) -> Result<()> {
    let bin_path = mihoro_bin_path()?;
    let snapshot = read_current_crontab()?;
    if !snapshot.existed && snapshot.content.is_empty() {
        println!(
            "{} Auto-update disabled (no active cron job)",
            prefix.yellow()
        );
        return Ok(());
    }

    let cleaned = remove_managed_cron_content(&snapshot.content, &bin_path);
    if cleaned == snapshot.content {
        println!("{} Auto-update disabled", prefix.green().bold());
        return Ok(());
    }

    let _backup_path = write_crontab_backup(&snapshot)?;
    write_crontab_content(&cleaned)?;
    if let Err(err) = verify_auto_update_disabled(&bin_path) {
        if let Err(rollback_err) = restore_crontab(&snapshot) {
            anyhow::bail!("{}; rollback failed: {}", err, rollback_err);
        }
        return Err(err);
    }

    let crontab_file = crontab_path();
    if crontab_file.exists() {
        let _ = fs::remove_file(&crontab_file);
    }

    println!("{} Auto-update disabled", prefix.green().bold());
    Ok(())
}

/// Format Unix timestamp to local datetime string using date command
fn format_datetime(secs: u64) -> String {
    let output = Command::new("date")
        .arg("-d")
        .arg(format!("@{}", secs))
        .arg("+%Y-%m-%d %H:%M:%S")
        .output();

    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => format!("<unknown: {} secs>", secs),
    }
}

/// Get current cron status
pub fn get_cron_status(_prefix: &str, mihomo_config_path: &str) -> Result<()> {
    let bin_path = mihoro_bin_path()?;
    let snapshot = read_current_crontab()?;

    match find_mihoto_cron_entry(&snapshot.content, &bin_path) {
        Some(cron_entry) => {
            println!("{} Auto-update is enabled", "status:".green().bold());
            println!("{} {}", "->".dimmed(), cron_entry.dimmed());
        }
        None => {
            println!("{} Auto-update is disabled", "status:".yellow().bold());
            return Ok(());
        }
    }

    // Show last updated time from mihomo config file
    let config_path = Path::new(mihomo_config_path);
    if let Ok(metadata) = fs::metadata(config_path) {
        if let Ok(modified) = metadata.modified() {
            use std::time::UNIX_EPOCH;
            if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                let secs = duration.as_secs();
                let datetime = format_datetime(secs);
                println!("{} Last updated: {}", "->".dimmed(), datetime.dimmed());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_cron_entry() {
        let entry = generate_cron_entry(12).unwrap();
        assert!(entry.contains("0 */12 * * *"));
        assert!(entry.contains("update"));
    }

    #[test]
    fn test_quote_cron_command_wraps_paths_and_escapes_single_quotes() {
        assert_eq!(
            quote_cron_command("/opt/Mihoro Bin/mihoro"),
            "'/opt/Mihoro Bin/mihoro'"
        );
        assert_eq!(
            quote_cron_command("/opt/mihoro's bin/mihoro"),
            "'/opt/mihoro'\\''s bin/mihoro'"
        );
    }

    #[test]
    fn test_build_managed_cron_block_uses_stable_markers() {
        let block = build_managed_cron_block(6, "/usr/local/bin/mihoro");

        assert_eq!(
            block,
            "# mihoto: begin auto-update\n\
0 */6 * * * '/usr/local/bin/mihoro' update\n\
# mihoto: end auto-update\n"
        );
    }

    #[test]
    fn test_merge_managed_cron_block_preserves_unrelated_entries() {
        let existing = "\
27 16 * * * \"/root/.acme.sh\"/acme.sh --cron --home \"/root/.acme.sh\" > /dev/null
0 3 * * * /opt/backup/run.sh
";
        let merged = merge_managed_cron_block(
            existing,
            &build_managed_cron_block(12, "/root/.local/bin/mihoro"),
            "/root/.local/bin/mihoro",
        );

        assert!(merged.contains("\"/root/.acme.sh\"/acme.sh --cron"));
        assert!(merged.contains("/opt/backup/run.sh"));
        assert_eq!(count_managed_cron_blocks(&merged), 1);
        assert!(merged.contains("# mihoto: begin auto-update"));
        assert!(merged.contains("0 */12 * * * '/root/.local/bin/mihoro' update"));
        assert!(merged.contains("# mihoto: end auto-update"));
    }

    #[test]
    fn test_merge_managed_cron_block_replaces_existing_block() {
        let existing = "\
# existing user note
# mihoto: begin auto-update
0 */6 * * * '/root/.local/bin/mihoro' update
# mihoto: end auto-update
15 4 * * * /opt/backup/run.sh
";
        let merged = merge_managed_cron_block(
            existing,
            &build_managed_cron_block(12, "/root/.local/bin/mihoro"),
            "/root/.local/bin/mihoro",
        );

        assert_eq!(count_managed_cron_blocks(&merged), 1);
        assert!(merged.contains("# existing user note"));
        assert!(merged.contains("15 4 * * * /opt/backup/run.sh"));
        assert!(merged.contains("0 */12 * * * '/root/.local/bin/mihoro' update"));
        assert!(!merged.contains("0 */6 * * * '/root/.local/bin/mihoro' update"));
    }

    #[test]
    fn test_remove_managed_cron_content_preserves_unrelated_entries() {
        let existing = "\
# keep this comment
# mihoto: begin auto-update
0 */12 * * * '/root/.local/bin/mihoro' update
# mihoto: end auto-update
27 16 * * * /root/.acme.sh/acme.sh --cron > /dev/null
";
        let cleaned = remove_managed_cron_content(existing, "/root/.local/bin/mihoro");

        assert_eq!(
            cleaned,
            "# keep this comment\n27 16 * * * /root/.acme.sh/acme.sh --cron > /dev/null\n"
        );
    }

    #[test]
    fn test_marker_text_inside_user_command_is_preserved() {
        let existing = "10 1 * * * /usr/bin/printf '# mihoto: begin auto-update'\n";
        let cleaned = remove_managed_cron_content(existing, "/root/.local/bin/mihoro");

        assert_eq!(cleaned, existing);
    }

    #[test]
    fn test_no_crontab_message_is_successfully_classified() {
        assert!(is_no_crontab_message("no crontab for pectics\n"));
        assert!(is_no_crontab_message("crontab: no crontab for root\n"));
        assert!(!is_no_crontab_message("crontab: permission denied\n"));
    }

    #[test]
    fn test_non_no_crontab_list_failure_is_error() {
        let err = snapshot_from_failed_crontab_list("crontab: permission denied\n")
            .expect_err("permission failures must not be treated as an empty crontab");

        assert!(err.to_string().contains("Failed to read crontab"));
    }

    #[test]
    fn test_disable_on_no_crontab_is_success_path() {
        let cleaned = remove_managed_cron_content("", "/root/.local/bin/mihoro");
        assert!(cleaned.is_empty());
        assert_eq!(count_managed_cron_blocks(&cleaned), 0);
    }

    #[test]
    fn test_legacy_current_binary_entry_is_migrated_but_similar_lines_remain() {
        let existing = "\
0 */6 * * * /root/.local/bin/mihoro update
3 2 * * * /root/.local/bin/mihoro update --dry-run
4 2 * * * /opt/other/mihoro update
";
        let merged = merge_managed_cron_block(
            existing,
            &build_managed_cron_block(12, "/root/.local/bin/mihoro"),
            "/root/.local/bin/mihoro",
        );

        assert!(!merged.contains("0 */6 * * * /root/.local/bin/mihoro update\n"));
        assert!(merged.contains("/root/.local/bin/mihoro update --dry-run"));
        assert!(merged.contains("/opt/other/mihoro update"));
        assert_eq!(count_managed_cron_blocks(&merged), 1);
    }
}
