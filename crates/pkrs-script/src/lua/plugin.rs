//! `plugin.define()`: binds a plugin's manifest to its `_ENV` (§9.4, §9.5).

use mlua::Table;
use pkrs_core::PluginManifest;

use super::{LuaScriptHost, engine, hook_order};
use crate::error::Result;
use crate::host::ScriptOwner;
use crate::plugin::PluginHost;

/// Builds the `plugin` module around the plugin's own table `P`.
const PLUGIN_MODULE: &str = r#"
local P = ...
return { define = function() return P end }
"#;

impl PluginHost for LuaScriptHost {
    fn bind_plugin(&mut self, manifest: &PluginManifest) -> Result<()> {
        let owner = ScriptOwner::plugin(manifest.id());
        // Bound in load order, so this is the plugin's rank in hook chains (§9.4).
        hook_order::bind_plugin_rank(&self.lua, manifest.id());
        let env = self.env(&owner)?;
        let info = &manifest.plugin;
        let p: Table = self.lua.create_table().map_err(engine)?;
        let fill = || -> mlua::Result<()> {
            p.raw_set("id", info.id.as_str())?;
            p.raw_set("name", info.name.as_str())?;
            p.raw_set("version", info.version.to_string())?;
            p.raw_set("api", info.api)?;
            p.raw_set("description", info.description.as_str())?;
            p.raw_set("save", self.lua.create_table()?)?;
            Ok(())
        };
        fill().map_err(engine)?;
        let module: Table = self
            .lua
            .load(PLUGIN_MODULE)
            .set_name("=pkrs-plugin")
            .call(p)
            .map_err(engine)?;
        env.raw_set("plugin", module).map_err(engine)
    }
}
