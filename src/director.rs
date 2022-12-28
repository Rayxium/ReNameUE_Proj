use walkdir::WalkDir;

use crate::{
    changesets::{generate_blueprint_changeset, generate_code_changeset},
    engine::Engine,
    logger::Log,
};
use std::{
    fs,
    io::{stdin, Read},
    path::{Path, PathBuf},
};

/// Takes a result and returns its inner
/// value if it is ok. In the case of error,
/// logs the error and returns from the function.
macro_rules! ok_or_quit {
    ( $e:expr ) => {
        match $e {
            Ok(t) => t,
            Err(e) => {
                Log::error(e);
                return;
            }
        }
    };
}

enum ProjectType {
    Blueprint,
    Code,
}

pub fn start_interactive_rename() {
    Log::check_support_for_colors();
    Log::header("Welcome to Renom");
    Log::header("Project Details");
    Log::basic("Tell us a little about your project.");

    Log::prompt("Project root");
    let project_root = ok_or_quit!(request_project_root());

    let original_name = ok_or_quit!(infer_original_project_name(&project_root));
    Log::basic(format!("Project original name: {}", &original_name));

    Log::prompt("Project final name");
    let final_name = ok_or_quit!(request_final_project_name(&original_name));

    let project_type = detect_project_type(&project_root);
    match project_type {
        ProjectType::Blueprint => Log::basic("Blueprint project detected."),
        ProjectType::Code => Log::basic("Code project detected."),
    }

    Log::header("Staging");
    let backup_dir = ok_or_quit!(create_backup_dir(&project_root));
    Log::basic(format!(
        "Created backup directory at {}",
        backup_dir.to_str().unwrap()
    ));

    let changeset = match project_type {
        ProjectType::Blueprint => {
            generate_blueprint_changeset(&original_name, &final_name, &project_root)
        }
        ProjectType::Code => generate_code_changeset(
            &original_name,
            &final_name,
            &project_root,
            ok_or_quit!(get_files_including_api_macro(&project_root, &original_name)),
        ),
    };

    Log::header("Application");
    let mut engine = Engine::new();
    match engine.execute(changeset, &backup_dir) {
        Ok(_) => {
            Log::header("Cleanup");
            match project_type {
                ProjectType::Blueprint => {
                    Log::basic("Nothing to clean up for Blueprint project.");
                }
                ProjectType::Code => {
                    Log::basic("Though not strictly necessary, it is a good idea to clean up outdated Saved, Intermediate, and Binaries folders.\nShall we go ahead and do so for you?");
                    Log::prompt("[Y]es/[N]o");
                    if request_cleanup() {
                        ok_or_quit!(cleanup(&project_root.with_file_name(&final_name)));
                    } else {
                        Log::basic("Cleanup skipped.");
                    }
                }
            }

            Log::header("Success");
            Log::basic("Project successfully renamed.");
        }
        Err(err) => {
            Log::error(err);
            Log::header("Recovery");
            Log::basic("Looks like things did not work out as planned. Would you like to revert the changes made so far?");
            Log::prompt("[Y]es/[N]o");
            if request_recover() {
                ok_or_quit!(engine.revert());
            } else {
                Log::basic("Recovery skipped.");
            }
        }
    }

    Log::newline();
    Log::prompt("Press Enter to exit.");
    let _ = stdin().read(&mut [0u8]);
}

/// Request the project root directory from the user.
fn request_project_root() -> Result<PathBuf, String> {
    let mut buffer = String::new();
    let root = stdin()
        .read_line(&mut buffer)
        .map(|_| PathBuf::from(buffer.trim()))
        .map_err(|err| err.to_string())?;
    root.is_dir()
        .then(|| root)
        .ok_or_else(|| "Provided path was not a directory".to_owned())
}

/// Infer the project's original name from the project root.
fn infer_original_project_name(project_root: &Path) -> Result<String, String> {
    let project_descriptor = fs::read_dir(project_root)
        .map_err(|err| err.to_string())?
        .find_map(|entry| {
            if let Ok(entry) = entry {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "uproject" {
                        return Some(path);
                    }
                }
            }
            None
        });

    project_descriptor
        .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
        .ok_or_else(|| "Failed to find .uproject file in project root".to_owned())
}

/// Request final project name from the user.
fn request_final_project_name(original_name: &str) -> Result<String, String> {
    let mut buffer = String::new();
    let final_name = stdin()
        .read_line(&mut buffer)
        .map(|_| String::from(buffer.trim()))
        .map_err(|err| err.to_string())?;

    if final_name.len() > 20 {
        return Err("Name is too long.".to_owned());
    }

    if final_name == original_name {
        return Err("Final name is identical to original name.".to_owned());
    }

    Ok(final_name)
}

/// Detect project type (Blueprint or C++) based on existence of
/// *Source* directory.
fn detect_project_type(project_root: &Path) -> ProjectType {
    if project_root.join("Source").is_dir() {
        ProjectType::Code
    } else {
        ProjectType::Blueprint
    }
}

/// Create a directory to store backup files in
fn create_backup_dir(project_root: &Path) -> Result<PathBuf, String> {
    let backup_dir = project_root.join(".renom/backup");
    fs::create_dir_all(&backup_dir).map_err(|err| err.to_string())?;
    Ok(backup_dir)
}

/// Get files that include the project API macro.
fn get_files_including_api_macro(
    project_root: &Path,
    original_name: &str,
) -> Result<Vec<PathBuf>, String> {
    let files: Vec<PathBuf> = WalkDir::new(project_root.join("Source").join(original_name))
        .into_iter()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().to_owned())
        .filter(|path| {
            let content = fs::read_to_string(path);
            content.is_ok()
                && content
                    .unwrap()
                    .contains(&format!("{}_API", original_name.to_uppercase()))
        })
        .filter_map(|path| {
            path.strip_prefix(project_root)
                .map(|path| path.to_owned())
                .ok()
        })
        .collect();

    Ok(files)
}

/// Request recover desired from the user.
fn request_recover() -> bool {
    let mut buffer = String::new();
    stdin().read_line(&mut buffer).unwrap();
    matches!(buffer.trim(), "y" | "Y" | "yes" | "Yes")
}

/// Request cleanup desired from the user.
fn request_cleanup() -> bool {
    let mut buffer = String::new();
    stdin().read_line(&mut buffer).unwrap();
    matches!(buffer.trim(), "y" | "Y" | "yes" | "Yes")
}

/// Cleanup *Saved*, *Intermediate*, and *Binaries* directories.
fn cleanup(project_root: &Path) -> Result<(), String> {
    Log::basic("Deleting Saved directory.");
    let saved_dir = project_root.join("Saved");
    if saved_dir.is_dir() {
        fs::remove_dir_all(saved_dir).map_err(|err| err.to_string())?;
    } else {
        Log::basic("Does not exist. Skipped.");
    }

    Log::basic("Deleting Intermediate directory.");
    let intermediate_dir = project_root.join("Intermediate");
    if intermediate_dir.is_dir() {
        fs::remove_dir_all(intermediate_dir).map_err(|err| err.to_string())?;
    } else {
        Log::basic("Does not exist. Skipped.");
    }

    Log::basic("Deleting Binaries directory.");
    let binaries_dir = project_root.join("Binaries");
    if binaries_dir.is_dir() {
        fs::remove_dir_all(binaries_dir).map_err(|err| err.to_string())?;
    } else {
        Log::basic("Does not exist. Skipped.");
    }

    Ok(())
}
