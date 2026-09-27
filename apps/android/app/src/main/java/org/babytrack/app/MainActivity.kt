package org.babytrack.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.compose.runtime.rememberCoroutineScope
import uniffi.babytrack_core_ffi.ActivityRow
import uniffi.babytrack_core_ffi.ActivityWhen
import uniffi.babytrack_core_ffi.ChildRow
import uniffi.babytrack_core_ffi.FamilyRef
import uniffi.babytrack_core_ffi.NativeLocalStore
import java.text.DateFormat
import java.util.Date
import java.util.TimeZone

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val database = filesDir.resolve("families.db")
        setContent {
            MaterialTheme {
                TrackerScreen(
                    store = remember { NativeLocalStore.open(database.absolutePath) },
                    readFile = { uri -> contentResolver.openInputStream(uri)?.use { it.readBytes() } },
                    writeFile = { uri, bytes ->
                        contentResolver.openOutputStream(uri)?.use { it.write(bytes) } ?: error("No output stream")
                    },
                )
            }
        }
    }
}

private fun ByteArray.key(): String = joinToString("") { "%02x".format(it) }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TrackerScreen(
    store: NativeLocalStore,
    readFile: (android.net.Uri) -> ByteArray?,
    writeFile: (android.net.Uri, ByteArray) -> Unit,
) {
    DisposableEffect(store) { onDispose { store.close() } }
    val scope = rememberCoroutineScope()
    var version by remember { mutableStateOf(0) }
    var families by remember { mutableStateOf<List<FamilyRef>>(emptyList()) }
    var children by remember { mutableStateOf<List<ChildRow>>(emptyList()) }
    var entries by remember { mutableStateOf<List<ActivityRow>>(emptyList()) }
    var selectedFamily by remember { mutableStateOf<String?>(null) }
    var selectedChild by remember { mutableStateOf<String?>(null) }
    var childName by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    val errorText = stringResource(R.string.error)
    val savedText = stringResource(R.string.saved)
    val restoredText = stringResource(R.string.restored)
    var pendingBackup by remember { mutableStateOf<ByteArray?>(null) }
    val saveLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) scope.launch {
            runCatching { withContext(Dispatchers.IO) { writeFile(uri, pendingBackup ?: error("Missing backup")) } }
                .onSuccess { message = savedText }
                .onFailure { message = errorText }
            pendingBackup = null
        }
    }
    val restoreLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            runCatching {
                val restored = withContext(Dispatchers.IO) {
                    store.restore(readFile(uri) ?: error("Missing backup"), System.currentTimeMillis())
                }
                selectedFamily = restored.familyId.key()
                selectedChild = null
                version++
                message = restoredText
            }.onFailure { message = errorText }
        }
    }
    fun change(action: () -> Unit) {
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { action() } }
                .onSuccess { version++; message = null }
                .onFailure { message = errorText }
        }
    }
    LaunchedEffect(version, selectedFamily, selectedChild) {
        runCatching {
            withContext(Dispatchers.IO) {
                val all = store.families()
                val family = all.find { it.familyId.key() == selectedFamily } ?: all.firstOrNull()
                val kids = family?.let(store::children).orEmpty()
                val child = kids.find { it.id.key() == selectedChild } ?: kids.firstOrNull()
                val history = if (family != null && child != null) store.timeline(family, child.id) else emptyList()
                Triple(all, kids, history)
            }
        }.onSuccess { (all, kids, history) ->
            families = all
            selectedFamily = all.find { it.familyId.key() == selectedFamily }?.familyId?.key() ?: all.firstOrNull()?.familyId?.key()
            children = kids
            selectedChild = kids.find { it.id.key() == selectedChild }?.id?.key() ?: kids.firstOrNull()?.id?.key()
            entries = history
        }.onFailure { message = errorText }
    }
    val family = families.find { it.familyId.key() == selectedFamily }
    val child = children.find { it.id.key() == selectedChild }
    val filename = stringResource(R.string.backup_filename)

    Scaffold(topBar = { TopAppBar(title = { Text(stringResource(R.string.screen_title)) }) }) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(stringResource(R.string.local_only), style = MaterialTheme.typography.labelMedium)
            Text(stringResource(R.string.families), style = MaterialTheme.typography.titleLarge)
            families.forEachIndexed { index, item ->
                FilterChip(
                    selected = item.familyId.key() == selectedFamily,
                    onClick = { selectedFamily = item.familyId.key(); selectedChild = null },
                    label = { Text(stringResource(R.string.family_number, index + 1)) },
                )
            }
            OutlinedButton(onClick = {
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { store.createFamily(System.currentTimeMillis()) } }
                        .onSuccess { created ->
                            selectedFamily = created.familyId.key()
                            selectedChild = null
                            version++
                            message = null
                        }.onFailure { message = errorText }
                }
            }) { Text(stringResource(R.string.new_family)) }

            if (family != null) {
                Text(stringResource(R.string.children), style = MaterialTheme.typography.titleLarge)
                if (children.isEmpty()) Text(stringResource(R.string.no_children))
                children.forEach { item ->
                    FilterChip(
                        selected = item.id.key() == selectedChild,
                        onClick = { selectedChild = item.id.key() },
                        label = { Text(item.name) },
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = childName,
                        onValueChange = { childName = it },
                        label = { Text(stringResource(R.string.child_name)) },
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                    Button(enabled = childName.isNotBlank(), onClick = {
                        val name = childName.trim()
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) { store.addChild(family, name, System.currentTimeMillis()) } }
                                .onSuccess { created ->
                                    selectedChild = created.key()
                                    childName = ""
                                    version++
                                    message = null
                                }.onFailure { message = errorText }
                        }
                    }) { Text(stringResource(R.string.add_child)) }
                }

                if (child != null) {
                    Text(stringResource(R.string.log_diaper), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(1u.toUByte() to R.string.wet, 2u.toUByte() to R.string.dirty, 3u.toUByte() to R.string.both).forEach { (kind, label) ->
                            Button(onClick = { change { store.logDiaper(family, child.id, kind, nowTime()) } }) {
                                Text(stringResource(label))
                            }
                        }
                    }
                    Text(stringResource(R.string.log_bottle), style = MaterialTheme.typography.titleLarge)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = amount,
                            onValueChange = { amount = it.filter(Char::isDigit) },
                            label = { Text(stringResource(R.string.amount_ml)) },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.weight(1f),
                            singleLine = true,
                        )
                        Button(enabled = (amount.toLongOrNull() ?: 0) > 0, onClick = {
                            val ml = amount.toLongOrNull() ?: return@Button
                            change { store.logBottleMl(family, child.id, ml, 2u.toUByte(), nowTime()) }
                            amount = ""
                        }) { Text(stringResource(R.string.log_bottle)) }
                    }
                    Text(stringResource(R.string.timeline), style = MaterialTheme.typography.titleLarge)
                    if (entries.isEmpty()) Text(stringResource(R.string.no_entries))
                    entries.forEach { entry ->
                        val label = when {
                            entry.bottleMl != null -> stringResource(R.string.bottle, entry.bottleMl!!)
                            entry.diaperKind != null -> stringResource(R.string.diaper, when (entry.diaperKind!!.toInt()) {
                                1 -> stringResource(R.string.wet)
                                2 -> stringResource(R.string.dirty)
                                else -> stringResource(R.string.both)
                            })
                            else -> entry.kind
                        }
                        Card(Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(12.dp)) {
                                Text(label, fontWeight = FontWeight.SemiBold)
                                Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(entry.startUtcMs)))
                            }
                        }
                    }
                }

                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.backup_title), style = MaterialTheme.typography.titleLarge)
                Text(stringResource(R.string.backup_description))
                Button(onClick = {
                    scope.launch {
                        runCatching { withContext(Dispatchers.IO) { store.backup(family, System.currentTimeMillis()) } }
                            .onSuccess { pendingBackup = it; saveLauncher.launch(filename) }
                            .onFailure { message = errorText }
                    }
                }) { Text(stringResource(R.string.save_backup)) }
            }
            OutlinedButton(onClick = { restoreLauncher.launch(arrayOf("application/octet-stream", "*/*")) }) {
                Text(stringResource(R.string.restore_backup))
            }
            message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }
}

private fun nowTime(): ActivityWhen {
    val now = System.currentTimeMillis()
    return ActivityWhen(now, (TimeZone.getDefault().getOffset(now) / 60_000).toShort(), now)
}
