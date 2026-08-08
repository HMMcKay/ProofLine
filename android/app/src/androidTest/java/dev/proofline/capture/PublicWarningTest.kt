package dev.proofline.capture

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class PublicWarningTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun warningExplainsPublicCaptureBeforeConsent() {
        var accepted = false
        compose.setContent { PublicWarning { accepted = true } }

        compose.onNodeWithText("Opening this app records in public.").assertIsDisplayed()
        compose.onNodeWithText("There is no private mode.").assertIsDisplayed()
        compose.onNodeWithText("I understand — configure ProofLine").performClick()

        compose.runOnIdle { assertTrue(accepted) }
    }
}
