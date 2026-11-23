package io.ahara.rollinglocal.operator

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

@Composable
fun MenuItemsDialog(menuId: String, client: OperatorClient, dismiss: () -> Unit) {
    var result by remember(menuId) { mutableStateOf<MenuResults?>(null) }
    var draft by remember(menuId) { mutableStateOf<List<MenuItem>?>(null) }
    var message by remember(menuId) { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    suspend fun load() {
        busy = true
        try {
            val current = client.menuResults()
            if (current.extraction?.menuId?.let { it != menuId } == true) {
                result = null
                message = "A newer photo is published. Reopen its menu items."
            } else {
                result = current
                message = null
            }
        } catch (cancel: CancellationException) { throw cancel }
        catch (error: Exception) { message = error.message }
        finally { busy = false }
    }
    LaunchedEffect(menuId) { load() }
    val edits = draft
    AlertDialog(onDismissRequest = dismiss, title = { Text("Menu items") }, text = {
        Column(Modifier.heightIn(max = 540.dp).verticalScroll(rememberScrollState())) {
            result?.let { current ->
                Text(current.extraction?.status?.replaceFirstChar { it.uppercase() } ?: "Photo only")
                if (current.extraction?.status == "failed") {
                    Text("The photo is published. You can enter its items here.")
                    if (edits == null) TextButton({
                        scope.launch {
                            busy = true
                            try {
                                result = client.retryMenu(current.extraction)
                                message = null
                            } catch (cancel: CancellationException) { throw cancel }
                            catch (error: Exception) { message = error.message }
                            finally { busy = false }
                        }
                    }, enabled = !busy) { Text("Retry extraction") }
                }
                if (edits == null) {
                    for (item in current.extraction?.items.orEmpty()) {
                        HorizontalDivider()
                        Text(item.name)
                        item.description?.let { Text(it) }
                        item.priceText?.let { Text(it) }
                        if (item.tagIds.isNotEmpty()) Text(current.tags.filter { it.id in item.tagIds }.joinToString { it.label })
                    }
                    TextButton({ draft = current.extraction?.items.orEmpty() }, enabled = !busy) { Text("Edit items") }
                } else {
                    edits.forEachIndexed { index, item -> key(item.id) {
                        HorizontalDivider()
                        MenuItemFields(item, current.tags) { changed ->
                            draft = edits.toMutableList().apply { set(index, changed) }
                        }
                        Row {
                            TextButton({ draft = edits.toMutableList().apply { add(index - 1, removeAt(index)) } },
                                enabled = index > 0) { Text("Move up") }
                            TextButton({ draft = edits.filterNot { it.id == item.id } }) { Text("Remove") }
                        }
                    } }
                    TextButton({ draft = edits + MenuItem() }) { Text("Add item") }
                }
            }
            if (edits == null) TextButton({ scope.launch { load() } }, enabled = !busy) { Text("Refresh") }
            message?.let { Text(it) }
        }
    }, confirmButton = {
        if (edits == null) TextButton(dismiss) { Text("Done") }
        else TextButton({
            scope.launch {
                busy = true
                try {
                    result = client.saveMenu(MenuCorrection(menuId, result?.extraction?.revision, edits))
                    draft = null
                    message = "Menu items saved."
                } catch (cancel: CancellationException) { throw cancel }
                catch (error: Exception) {
                    message = if (error is ServerError && error.code == 409)
                        "The menu changed. Cancel edits and refresh before saving again." else error.message
                } finally { busy = false }
            }
        }, enabled = !busy && edits.none { it.name.isBlank() }) { Text("Save") }
    }, dismissButton = {
        if (edits != null) TextButton({ draft = null }, enabled = !busy) { Text("Cancel edits") }
    })
}

@Composable
private fun MenuItemFields(item: MenuItem, tags: List<MenuTag>, update: (MenuItem) -> Unit) {
    var showTags by remember { mutableStateOf(false) }
    OutlinedTextField(item.name, { update(item.copy(name = it)) }, Modifier.fillMaxWidth(), label = { Text("Item name") })
    OutlinedTextField(item.description.orEmpty(), { update(item.copy(description = it.ifBlank { null })) },
        Modifier.fillMaxWidth(), label = { Text("Description") })
    OutlinedTextField(item.priceText.orEmpty(), { update(item.copy(priceText = it.ifBlank { null })) },
        Modifier.fillMaxWidth(), label = { Text("Price as printed") })
    TextButton({ showTags = !showTags }) { Text("Tags (${item.tagIds.size})") }
    if (showTags) tags.forEach { tag ->
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(tag.id in item.tagIds, { selected ->
                update(item.copy(tagIds = if (selected) item.tagIds + tag.id else item.tagIds - tag.id))
            })
            Text(tag.label)
        }
    }
}
