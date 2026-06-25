package net.thorminate.pojoc

import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.extensions.PluginId
import org.jetbrains.plugins.textmate.api.TextMateBundleProvider

class PojocTextMateBundleProvider : TextMateBundleProvider {

    override fun getBundles(): List<TextMateBundleProvider.PluginBundle> {
        return listOf(
            TextMateBundleProvider.PluginBundle(
                "pojoc",
                PluginManagerCore.getPlugin(PluginId.getId("net.thorminate.pojoc"))
                    !!.pluginPath.resolve("textmate/pojoc")
            )
        )
    }
}