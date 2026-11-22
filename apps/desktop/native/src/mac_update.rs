use objc2_foundation::{NSBundle, NSString};
use rustix::fs::{CWD, RenameFlags, renameat_with};
use std::{env, fs, io, os::unix::fs::MetadataExt, path::PathBuf};

pub(super) fn exchange() -> io::Result<()> {
    let invalid = || io::Error::other("The installed app changed. Reopen it before updating.");
    let exe = env::current_exe()?.canonicalize()?;
    let installed = exe.ancestors().nth(3).ok_or_else(invalid)?;
    if !exe.ends_with("Contents/MacOS/t3-code")
        || installed.extension().is_none_or(|ext| ext != "app")
    {
        return Err(invalid());
    }
    let mut args = env::args_os().skip(2);
    let replacement = PathBuf::from(args.next().ok_or_else(invalid)?).canonicalize()?;
    let metadata = fs::metadata(installed)?;
    let relative = replacement
        .strip_prefix(installed.parent().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let parts: Vec<_> = relative.iter().collect();
    if parts.len() != 3
        || !parts[0].to_string_lossy().starts_with(".t3-update-")
        || parts[1] != "unpacked"
        || replacement.extension().is_none_or(|ext| ext != "app")
        || args.next().as_deref() != Some(metadata.dev().to_string().as_ref())
        || args.next().as_deref() != Some(metadata.ino().to_string().as_ref())
        || args.next().is_some()
    {
        return Err(invalid());
    }
    let architectures = |path: &std::path::Path| {
        path.to_str()
            .and_then(|p| NSBundle::bundleWithPath(&NSString::from_str(p)))
            .and_then(|bundle| bundle.executableArchitectures())
            .filter(|cpus| cpus.count() > 0)
            .ok_or_else(|| io::Error::other("Could not read the app’s executable architectures."))
    };
    let available = architectures(&replacement)?;
    if !architectures(installed)?
        .iter()
        .all(|cpu| available.containsObject(&cpu))
    {
        return Err(io::Error::other(
            "The update does not support this installation’s architectures.",
        ));
    }
    renameat_with(CWD, replacement, CWD, installed, RenameFlags::EXCHANGE)?;
    Ok(())
}
