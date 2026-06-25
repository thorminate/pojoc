package net.thorminate.pojoc

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.SystemInfo
import com.redhat.devtools.lsp4ij.server.OSProcessStreamConnectionProvider

class PojocLanguageServer(private val project: Project) : OSProcessStreamConnectionProvider() {

    init {
        val pojocBin = findPojocBinary()
        val cmd = GeneralCommandLine(pojocBin).apply {
            withWorkDirectory(project.basePath)
            withParentEnvironmentType(GeneralCommandLine.ParentEnvironmentType.CONSOLE)
        }
        setCommandLine(cmd)
    }

    private fun findPojocBinary(): String {
        return "C:\\dev\\Rust\\pojoc\\target\\debug\\pojoc-lsp.exe"
    }
}