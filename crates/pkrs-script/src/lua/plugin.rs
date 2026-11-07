//! `plugin.define()`: binds a plugin's manifest to its `_ENV` (§9.4, §9.5). `P.save` and
//! `plugin.save` are the same table: the plugin's save namespace (`plugin.save.<key>`).

use mlua::Table;
use pkrs_core::PluginManifest;

use super::registry::with_regs;
use super::{LuaScriptHost, engine, hook_order};
use crate::error::Result;
use crate::host::ScriptOwner;
use crate::plugin::PluginHost;
use pkrs_data::ContentDb;

/// Builds the `plugin` module around the plugin's own table `P`.
const PLUGIN_MODULE: &str = r#"
local P = ...
return { define = function() return P end, save = P.save }
"#;

impl PluginHost for LuaScriptHost {
    fn bind_plugin(&mut self, manifest: &PluginManifest) -> Result<()> {
        let owner = ScriptOwner::plugin(manifest.id());
        // Bound in load order, so this is the plugin's rank in hook chains (§9.4).
        hook_order::bind_plugin_rank(&self.lua, manifest.id());
        let env = self.env(&owner)?;
        let info = &manifest.plugin;
        let p: Table = self.lua.create_table().map_err(engine)?;
        // One save table per plugin, kept across re-binds (hot reload) and read by
        // `plugin_save` (§9.4, §12).
        let existing =
            with_regs(&self.lua, |r| r.saves.get(info.id.as_str()).cloned()).map_err(engine)?;
        let save = match existing {
            Some(t) => t,
            None => {
                let t = self.lua.create_table().map_err(engine)?;
                with_regs(&self.lua, |r| r.saves.insert(info.id.clone(), t.clone()))
                    .map_err(engine)?;
                t
            }
        };
        let fill = || -> mlua::Result<()> {
            p.raw_set("id", info.id.as_str())?;
            p.raw_set("name", info.name.as_str())?;
            p.raw_set("version", info.version.to_string())?;
            p.raw_set("api", info.api)?;
            p.raw_set("description", info.description.as_str())?;
            p.raw_set("save", save)?;
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

    fn set_content(&mut self, db: ContentDb) {
        if with_regs(&self.lua, |r| r.content = db).is_err() {
            tracing::error!(target: "data", "content database could not be installed");
        }
    }
}
