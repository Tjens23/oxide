/// Filename of the npm package manifest.
pub const PACKAGE_JSON: &str = "package.json";

/// Filename of the oxide lock-file written into each project and cache entry.
pub const OXIDE_LOCK: &str = "oxide-lock.json";

/// Directory that Node.js uses to resolve installed packages.
pub const NODE_MODULES: &str = "node_modules";

/// Directory inside `node_modules` that holds executable shims.
pub const BIN_DIR: &str = ".bin";

/// Sub-directory inside the OS cache folder where oxide stores downloaded packages.
pub const CACHE_SUBDIR: &str = "node-cache";

/// Filename of the per-user oxide config file.
pub const CONFIG_FILE: &str = "config.json";

/// Sub-directory inside the oxide config folder for globally installed packages.
pub const GLOBAL_MODULES_SUBDIR: &str = "global/node_modules";

/// Sub-directory inside the oxide config folder where global binary symlinks are placed.
pub const GLOBAL_BIN_SUBDIR: &str = "bin";
