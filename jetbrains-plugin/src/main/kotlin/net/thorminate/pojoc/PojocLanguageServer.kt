package net.thorminate.pojoc

import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.SystemInfo
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import java.io.File

class PojocLanguageServer(project: Project) : ProcessStreamConnectionProvider(
    listOf(findPojocBinary()),
    project.basePath
) {

    companion object {
        private fun findPojocBinary(): String {
            val pluginId = PluginId.getId("net.thorminate.pojoc")
            val plugin = PluginManagerCore.getPlugin(pluginId)
                ?: throw IllegalStateException("Pojoc plugin descriptor could not be resolved.")

            val binDir = plugin.pluginPath.resolve("bin").toFile()
            if (!binDir.exists() || !binDir.isDirectory) {
                throw IllegalStateException("Pojoc 'bin' directory is missing: ${binDir.absolutePath}")
            }

            val arch = when {
                SystemInfo.isAarch64 -> "aarch64"
                SystemInfo.is64Bit -> "x86_64"
                else -> throw UnsupportedOperationException("Unsupported CPU architecture: ${System.getProperty("os.arch")}")
            }

            val osAndEnv = when {
                SystemInfo.isWindows -> "pc-windows-msvc"
                SystemInfo.isMac -> "apple-darwin"
                SystemInfo.isLinux -> "unknown-linux-gnu"
                else -> throw UnsupportedOperationException("Unsupported OS: ${SystemInfo.OS_NAME}")
            }

            val extension = if (SystemInfo.isWindows) ".exe" else ""
            val targetPattern = "$arch-$osAndEnv$extension"

            val binaryFile = binDir.listFiles()?.find { file ->
                file.name.startsWith("pojoc-lsp-") && file.name.endsWith(targetPattern)
            } ?: throw IllegalStateException("Executable binary matching configuration pattern '$targetPattern' not found in ${binDir.absolutePath}")

            if (!SystemInfo.isWindows && !binaryFile.canExecute()) {
                val success = binaryFile.setExecutable(true, false)
                if (!success) {
                    throw IllegalStateException("Failed to apply executable permissions to: ${binaryFile.absolutePath}")
                }
            }

            return binaryFile.absolutePath
        }
    }
}