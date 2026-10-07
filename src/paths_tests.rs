use super::*;
use std::os::unix::fs::symlink;

#[test]
fn private_directories_reject_symlinks_and_tighten_permissions() {
    let root = std::env::temp_dir().join(format!("ytfast-paths-{:032x}", fastrand::u128(..)));
    private_dir(&root).unwrap();
    let owned = root.join("owned");
    std::fs::create_dir(&owned).unwrap();
    std::fs::set_permissions(&owned, std::fs::Permissions::from_mode(0o755)).unwrap();
    private_dir(&owned).unwrap();
    assert_eq!(std::fs::metadata(&owned).unwrap().mode() & 0o777, 0o700);
    let link = root.join("link");
    symlink(&owned, &link).unwrap();
    assert!(private_dir(&link).is_err());
    let file = root.join("file");
    write_atomic(&file, b"first").unwrap();
    write_atomic(&file, b"second").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"second");
    assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    assert!(private_dir(&file).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
