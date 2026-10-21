allprojects {
    repositories {
        google()
        mavenCentral()
        // SDK do delonix-push (Apache-2.0), publicado com `./gradlew publishToMavenLocal` em delonix-push/sdk/android.
        mavenLocal()
        // Linphone SDK (spike da Fase 0, ADR-0022): AGPLv3 ou licença comercial. Só entra em
        // builds de DEBUG (ver app/build.gradle.kts): nenhum binário de release leva código AGPL.
        maven {
            url = uri("https://download.linphone.org/maven_repository")
            content { includeGroup("org.linphone.no-video") }
        }
    }
}

val newBuildDir: Directory =
    rootProject.layout.buildDirectory
        .dir("../../build")
        .get()
rootProject.layout.buildDirectory.value(newBuildDir)

subprojects {
    val newSubprojectBuildDir: Directory = newBuildDir.dir(project.name)
    project.layout.buildDirectory.value(newSubprojectBuildDir)
}
subprojects {
    project.evaluationDependsOn(":app")
}

tasks.register<Delete>("clean") {
    delete(rootProject.layout.buildDirectory)
}
