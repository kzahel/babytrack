package org.babytrack.app

import android.app.Application
import androidx.compose.foundation.ScrollState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.Density
import com.github.takahirom.roborazzi.captureRoboImage
import java.io.File
import java.util.Locale
import java.util.TimeZone
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.BeforeClass
import org.junit.Rule
import org.junit.Test
import org.junit.rules.ExternalResource
import org.junit.rules.RuleChain
import org.junit.runner.RunWith
import org.robolectric.ParameterizedRobolectricTestRunner
import org.robolectric.ParameterizedRobolectricTestRunner.Parameters
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Actual production composables; no MainActivity, native library, store, or relay. */
@RunWith(ParameterizedRobolectricTestRunner::class)
@Config(sdk = [35], qualifiers = "en-rUS-w412dp-h915dp-xhdpi", application = Application::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ScreenGalleryTest(private val caseId: String, private val variant: String) {
    private val viewport = when (System.getProperty("babytrack.gallery.viewport", "phone")) {
        "phone" -> Viewport("phone", 412, 915)
        "compact" -> Viewport("compact", 360, 800)
        else -> error("Use galleryViewport=phone or galleryViewport=compact")
    }
    private val compose = createComposeRule()
    @get:Rule
    val rules: RuleChain = RuleChain.outerRule(object : ExternalResource() {
        override fun before() {
            // Configure display metrics before Compose launches its test Activity.
            RuntimeEnvironment.setQualifiers("en-rUS-w${viewport.widthDp}dp-h${viewport.heightDp}dp-xhdpi")
        }
    }).around(compose)

    private data class Viewport(val name: String, val widthDp: Int, val heightDp: Int)
    private val previousZone = TimeZone.getDefault()
    private val previousLocale = Locale.getDefault()

    @Before
    fun fixedEnvironment() {
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        Locale.setDefault(Locale.US)
    }

    @After
    fun restoreEnvironment() {
        TimeZone.setDefault(previousZone)
        Locale.setDefault(previousLocale)
    }

    @Test
    fun render() {
        val fixture = ScreenFixtures.cases.single { it.id == caseId }
        val dark = variant.startsWith("dark")
        val fontScale = if (variant.endsWith("large")) 1.5f else 1f
        val scroll = ScrollState(0)
        val output = File(requireNotNull(System.getProperty("babytrack.gallery.output")))
        check(output.mkdirs() || output.isDirectory)
        compose.setContent {
            CompositionLocalProvider(LocalDensity provides Density(IMAGE_DENSITY, fontScale)) {
                BabytrackTheme(darkTheme = dark) { FixtureScreen(fixture, scroll) }
            }
        }
        compose.waitForIdle()
        val pages = JSONArray()
        var page = 0
        while (true) {
            val filename = "$caseId--$variant--$page.png"
            compose.onRoot().captureRoboImage(File(output, filename).absolutePath)
            pages.put(JSONObject().put("file", filename).put("scrollDp", scroll.value / IMAGE_DENSITY))
            val current = scroll.value
            val maximum = scroll.maxValue
            if (current >= maximum) break
            check(page < 30) { "Unexpectedly long fixture: $caseId ($maximum px)" }
            compose.runOnIdle {
                runBlocking { scroll.scrollTo((current + (600 * IMAGE_DENSITY).toInt()).coerceAtMost(maximum)) }
            }
            compose.waitForIdle()
            page++
        }
        File(output, "$caseId--$variant.json")
            .writeText(
                JSONObject()
                    .put("id", caseId)
                    .put("label", fixture.label)
                    .put("variant", variant)
                    .put("fontScale", fontScale)
                    .put("dark", dark)
                    .put("viewport", viewport.name)
                    .put("widthDp", viewport.widthDp)
                    .put("heightDp", viewport.heightDp)
                    .put("locale", "en-US")
                    .put("timeZone", "UTC")
                    .put("clock", "2026-09-29T14:00:00Z")
                    .put("api", 35)
                    .put("density", IMAGE_DENSITY)
                    .put("operatingSystem", System.getProperty("os.name"))
                    .put("javaVersion", System.getProperty("java.version"))
                    .put("pages", pages)
                    .toString(2)
            )
    }

    companion object {
        private const val IMAGE_DENSITY = 2f

        @JvmStatic
        @BeforeClass
        fun onlyWhenRecording() {
            assumeTrue(
                "Run recordScreenGallery to generate images",
                java.lang.Boolean.getBoolean("babytrack.gallery.record"),
            )
        }

        @JvmStatic
        @Parameters(name = "{0} / {1}")
        fun cases(): List<Array<String>> =
            ScreenFixtures.cases
                .filter {
                    it.id.contains(System.getProperty("babytrack.gallery.case", "").orEmpty())
                }
                .flatMap { fixture ->
                    listOf("dark", "light", "dark-large", "light-large").map {
                        arrayOf(fixture.id, it)
                    }
                }
    }
}
