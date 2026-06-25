package net.thorminate.pojoc

import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.client.LanguageClientImpl
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import org.jetbrains.annotations.NotNull

class PojocLanguageServerFactory : LanguageServerFactory {

    @NotNull
    override fun createConnectionProvider(@NotNull project: Project): StreamConnectionProvider {
        return PojocLanguageServer(project)
    }

    @NotNull
    override fun createLanguageClient(@NotNull project: Project): LanguageClientImpl {
        return LanguageClientImpl(project)
    }
}