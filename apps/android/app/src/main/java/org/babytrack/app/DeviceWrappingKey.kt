package org.babytrack.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Installation-bound key supplied to Rust only for local secret wrapping. */
internal class DeviceWrappingKey(context: Context) {
    private val appContext = context.applicationContext
    private val keyFile = AtomicFile(File(appContext.noBackupFilesDir, "installation-key.v1"))
    private val marker = appContext.getSharedPreferences("installation_key_state", Context.MODE_PRIVATE)

    @Synchronized
    fun loadOrCreate(): ByteArray {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (keyFile.baseFile.exists()) {
            val key = store.getKey(ALIAS, null) as? SecretKey
                ?: error("Installation key is no longer available")
            check(keyFile.baseFile.length() == FILE_BYTES.toLong()) {
                "Installation key file length is invalid"
            }
            val bytes = keyFile.readFully()
            check(bytes.size == FILE_BYTES && bytes.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) {
                "Installation key file is invalid"
            }
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, bytes.copyOfRange(5, 17)))
            cipher.updateAAD(AAD)
            val plain = cipher.doFinal(bytes.copyOfRange(17, bytes.size))
            check(plain.size == 32) { "Installation key length is invalid" }
            check(marker.edit().putBoolean("created", true).commit())
            return plain
        }
        check(!marker.getBoolean("created", false)) { "Installation key file is missing" }
        val key = (store.getKey(ALIAS, null) as? SecretKey) ?: KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            "AndroidKeyStore",
        ).run {
            init(
                KeyGenParameterSpec.Builder(
                    ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                ).setKeySize(256)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setRandomizedEncryptionRequired(true)
                    .build(),
            )
            generateKey()
        }
        val plain = ByteArray(32).also(SecureRandom()::nextBytes)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        cipher.updateAAD(AAD)
        val encoded = MAGIC + cipher.iv + cipher.doFinal(plain)
        check(encoded.size == FILE_BYTES)
        val stream = keyFile.startWrite()
        try {
            stream.write(encoded)
            keyFile.finishWrite(stream)
        } catch (error: Exception) {
            keyFile.failWrite(stream)
            plain.fill(0)
            throw error
        }
        check(marker.edit().putBoolean("created", true).commit())
        return plain
    }

    companion object {
        private const val ALIAS = "babytrack.installation.v1"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val FILE_BYTES = 5 + 12 + 32 + 16
        private val MAGIC = "BTWK1".toByteArray(Charsets.US_ASCII)
        private val AAD = "babytrack installation wrapping key v1".toByteArray(Charsets.US_ASCII)
    }
}
