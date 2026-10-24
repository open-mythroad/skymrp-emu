/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android

import android.Manifest
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.drawable.Icon as ShortcutIcon
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.Settings
import android.widget.Toast
import java.io.File
import java.util.Calendar
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.lifecycle.lifecycleScope
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.AlertDialog
import androidx.compose.material.Divider
import androidx.compose.material.DropdownMenu
import androidx.compose.material.DropdownMenuItem
import androidx.compose.material.FloatingActionButton
import androidx.compose.material.Icon
import androidx.compose.material.IconButton
import androidx.compose.material.IconToggleButton
import androidx.compose.material.LocalContentAlpha
import androidx.compose.material.LocalContentColor
import androidx.compose.material.MaterialTheme
import androidx.compose.material.Scaffold
import androidx.compose.material.Text
import androidx.compose.material.TextButton
import androidx.compose.material.TextField
import androidx.compose.material.TextFieldDefaults
import androidx.compose.material.TopAppBar
import androidx.compose.material.darkColors
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.ExitToApp
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.primarySurface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

private val SkyMRPColors = darkColors(
    primary = Color(0xFFF9A825),
    primaryVariant = Color(0xFFE18D0B),
    secondary = Color(0xFFF9A825),
    background = Color(0xFF181B1E),
    surface = Color(0xFF282D33),
    onPrimary = Color.White,
    onSecondary = Color.White,
    onBackground = Color.White,
    onSurface = Color.White,
)

private const val COPYRIGHT_START_YEAR = 2026
private const val ACTION_LAUNCH_MRP = "org.skymrp.android.action.LAUNCH_MRP"
private const val EXTRA_SHORTCUT_MRP_PATH = "org.skymrp.android.extra.SHORTCUT_MRP_PATH"
private const val EXTRA_SHORTCUT_MRP_TITLE = "org.skymrp.android.extra.SHORTCUT_MRP_TITLE"

class LauncherActivity : ComponentActivity() {
    private var storageRefreshToken by mutableStateOf(0)
    private val mythroadDirectory by lazy {
        File(Environment.getExternalStorageDirectory(), "mythroad")
    }
    private val mrpFileScanner by lazy {
        MrpFileScanner(mythroadDirectory, NativeMrpMetadataReader)
    }

    private val allFilesAccessLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) {
        storageRefreshToken++
    }

    private val storagePermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) {
        storageRefreshToken++
    }

    private val importMrpLauncher = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri == null) {
            return@registerForActivityResult
        }
        lifecycleScope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    importMrp(uri)
                }
            }.onSuccess { importedName ->
                if (importedName != null) {
                    storageRefreshToken++
                    Toast.makeText(
                        this@LauncherActivity,
                        "Imported $importedName",
                        Toast.LENGTH_SHORT,
                    ).show()
                }
            }.onFailure { error ->
                Toast.makeText(
                    this@LauncherActivity,
                    error.message ?: "Could not import MRP file",
                    Toast.LENGTH_SHORT,
                ).show()
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)

        if (savedInstanceState == null && launchMrpFromShortcut(intent)) {
            return
        }

        setContent {
            SkyMRPTheme {
                val refreshToken = storageRefreshToken
                var currentDirectory by remember {
                    mutableStateOf(mythroadDirectory)
                }
                var query by remember { mutableStateOf("") }
                var scanMode by remember { mutableStateOf(MrpScanMode.PARSED) }
                var items by remember { mutableStateOf(emptyList<MrpListItem>()) }
                var listResetToken by remember { mutableStateOf(0) }
                var displayedListScope by remember {
                    mutableStateOf<Triple<String, String, MrpScanMode>?>(null)
                }
                val hasStorageAccess = hasStorageAccess()
                val canNavigateUp = currentDirectory != mythroadDirectory
                val listScope = Triple(currentDirectory.path, query, scanMode)

                LaunchedEffect(
                    currentDirectory,
                    query,
                    scanMode,
                    refreshToken,
                    hasStorageAccess,
                ) {
                    if (query.isNotBlank()) {
                        delay(150)
                    }
                    val scannedItems = withContext(Dispatchers.IO) {
                        if (!hasStorageAccess) {
                            emptyList()
                        } else {
                            mrpFileScanner.scan(
                                directory = currentDirectory,
                                query = query,
                                mode = scanMode,
                            )
                        }
                    }
                    items = scannedItems
                    if (displayedListScope != listScope) {
                        displayedListScope = listScope
                        listResetToken++
                    }
                }

                BackHandler(enabled = canNavigateUp) {
                    currentDirectory.parentFile?.let { parent ->
                        currentDirectory = parent
                        query = ""
                    }
                }

                val visibleItems = if (canNavigateUp) {
                    listOf(
                        MrpListItem(
                            name = "..",
                            path = currentDirectory.parentFile!!.absolutePath,
                            vendor = "",
                            version = 0,
                            isDirectory = true,
                        ),
                    ) + items
                } else {
                    items
                }

                LauncherScreen(
                    items = visibleItems,
                    listResetToken = listResetToken,
                    title = if (canNavigateUp) currentDirectory.name else "SkyMRP",
                    query = query,
                    onQueryChange = { query = it },
                    scanMode = scanMode,
                    onScanModeChange = { scanMode = it },
                    canNavigateUp = canNavigateUp,
                    onNavigateUp = {
                        currentDirectory.parentFile?.let { parent ->
                            currentDirectory = parent
                            query = ""
                        }
                    },
                    versionName = packageManager
                        .getPackageInfo(packageName, 0)
                        .versionName
                        .orEmpty(),
                    onImport = {
                        importMrpLauncher.launch(arrayOf("*/*"))
                    },
                    onItemClick = { item ->
                        if (item.isDirectory) {
                            currentDirectory = File(item.path)
                            query = ""
                        } else {
                            launchMrp(item.path, item.name)
                        }
                    },
                    onRename = { item, newName ->
                        lifecycleScope.launch {
                            runCatching {
                                withContext(Dispatchers.IO) {
                                    renameMrp(item.path, newName)
                                }
                            }.onSuccess {
                                storageRefreshToken++
                                Toast.makeText(
                                    this@LauncherActivity,
                                    "Renamed to $newName",
                                    Toast.LENGTH_SHORT,
                                ).show()
                            }.onFailure { error ->
                                showOperationError(error, "Could not rename MRP file")
                            }
                        }
                    },
                    onDelete = { item ->
                        lifecycleScope.launch {
                            runCatching {
                                withContext(Dispatchers.IO) {
                                    deleteMrp(item.path)
                                }
                            }.onSuccess {
                                storageRefreshToken++
                                Toast.makeText(
                                    this@LauncherActivity,
                                    "Deleted ${item.name}",
                                    Toast.LENGTH_SHORT,
                                ).show()
                            }.onFailure { error ->
                                showOperationError(error, "Could not delete MRP file")
                            }
                        }
                    },
                    onAddShortcut = ::addMrpShortcut,
                    onExit = ::finishAndRemoveTask,
                )
            }
        }

        if (savedInstanceState == null && !hasStorageAccess()) {
            requestStorageAccess()
        }
    }

    private fun launchMrpFromShortcut(shortcutIntent: Intent): Boolean {
        if (shortcutIntent.action != ACTION_LAUNCH_MRP) {
            return false
        }
        val path = shortcutIntent.getStringExtra(EXTRA_SHORTCUT_MRP_PATH)
        val title = shortcutIntent.getStringExtra(EXTRA_SHORTCUT_MRP_TITLE)
        if (path == null || !launchMrp(path, title)) {
            Toast.makeText(
                this,
                "MRP file is no longer available",
                Toast.LENGTH_SHORT,
            ).show()
            return false
        }
        finish()
        return true
    }

    private fun launchMrp(path: String, title: String?): Boolean {
        val file = runCatching { resolveMrpFile(path) }.getOrNull() ?: return false
        val preferences = getSharedPreferences(
            EmulatorSettings.PREFERENCES,
            MODE_PRIVATE,
        )
        startActivity(
            Intent(this, MainActivity::class.java).apply {
                putExtra(MainActivity.EXTRA_MRP_PATH, file.path)
                putExtra(
                    MainActivity.EXTRA_MRP_TITLE,
                    title?.takeIf(String::isNotBlank) ?: file.name,
                )
                putExtra(
                    MainActivity.EXTRA_SCREEN_RESOLUTION,
                    preferences.getString(
                        EmulatorSettings.SCREEN_RESOLUTION,
                        EmulatorSettings.DEFAULT_SCREEN_RESOLUTION,
                    ) ?: EmulatorSettings.DEFAULT_SCREEN_RESOLUTION,
                )
                putExtra(
                    MainActivity.EXTRA_FULL_VM_ENABLED,
                    preferences.getBoolean(
                        EmulatorSettings.FULL_VM_ENABLED,
                        false,
                    ),
                )
                putExtra(
                    MainActivity.EXTRA_KEYPAD_MODE,
                    preferences.getString(
                        EmulatorSettings.KEYPAD_MODE,
                        EmulatorSettings.DEFAULT_KEYPAD_MODE,
                    ) ?: EmulatorSettings.DEFAULT_KEYPAD_MODE,
                )
            },
        )
        return true
    }

    private fun renameMrp(path: String, newName: String) {
        val source = resolveMrpFile(path)
        val normalizedName = newName.trim()
        require(normalizedName.isNotEmpty()) { "File name cannot be empty" }
        require(File(normalizedName).name == normalizedName) { "Invalid file name" }
        require(normalizedName.endsWith(".mrp", ignoreCase = true)) {
            "File name must end with .mrp"
        }
        val destination = File(source.parentFile, normalizedName).canonicalFile
        require(destination.parentFile == source.parentFile) { "Invalid file name" }
        require(!destination.exists()) { "A file with that name already exists" }
        check(source.renameTo(destination)) { "Could not rename MRP file" }
    }

    private fun deleteMrp(path: String) {
        val file = resolveMrpFile(path)
        check(file.delete()) { "Could not delete MRP file" }
    }

    private fun resolveMrpFile(path: String): File {
        val root = mythroadDirectory.canonicalFile
        val file = File(path).canonicalFile
        require(file.path.startsWith(root.path + File.separator)) {
            "MRP file is outside the mythroad directory"
        }
        require(file.isFile && file.extension.equals("mrp", ignoreCase = true)) {
            "MRP file is no longer available"
        }
        return file
    }

    private fun addMrpShortcut(item: MrpListItem) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            Toast.makeText(
                this,
                "Home screen shortcuts require Android 8 or newer",
                Toast.LENGTH_SHORT,
            ).show()
            return
        }

        val file = runCatching { resolveMrpFile(item.path) }.getOrElse { error ->
            showOperationError(error, "Could not add shortcut")
            return
        }
        val shortcutManager = getSystemService(ShortcutManager::class.java)
        if (!shortcutManager.isRequestPinShortcutSupported) {
            Toast.makeText(
                this,
                "Home screen shortcuts are not supported",
                Toast.LENGTH_SHORT,
            ).show()
            return
        }

        val shortcutId = UUID.nameUUIDFromBytes(
            file.path.toByteArray(Charsets.UTF_8),
        ).toString()
        val shortcutIntent = Intent(this, LauncherActivity::class.java).apply {
            action = ACTION_LAUNCH_MRP
            data = Uri.Builder()
                .scheme("skymrp")
                .authority("launch")
                .appendPath(shortcutId)
                .build()
            putExtra(EXTRA_SHORTCUT_MRP_PATH, file.path)
            putExtra(EXTRA_SHORTCUT_MRP_TITLE, item.name)
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK
        }
        val shortcut = ShortcutInfo.Builder(this, shortcutId)
            .setShortLabel(item.name)
            .setIcon(ShortcutIcon.createWithResource(this, R.drawable.ic_mrp_app))
            .setIntent(shortcutIntent)
            .build()
        if (!shortcutManager.requestPinShortcut(shortcut, null)) {
            Toast.makeText(
                this,
                "Could not add shortcut",
                Toast.LENGTH_SHORT,
            ).show()
        }
    }

    private fun showOperationError(error: Throwable, fallbackMessage: String) {
        Toast.makeText(
            this,
            error.message ?: fallbackMessage,
            Toast.LENGTH_SHORT,
        ).show()
    }

    private fun hasStorageAccess(): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager()
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            return true
        }
        return checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) ==
            PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
            PackageManager.PERMISSION_GRANTED
    }

    private fun requestStorageAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val appSettings = Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:$packageName"),
            )
            try {
                allFilesAccessLauncher.launch(appSettings)
            } catch (_: Exception) {
                allFilesAccessLauncher.launch(
                    Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION),
                )
            }
            return
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            storagePermissionLauncher.launch(
                arrayOf(
                    Manifest.permission.READ_EXTERNAL_STORAGE,
                    Manifest.permission.WRITE_EXTERNAL_STORAGE,
                ),
            )
        }
    }

    private fun importMrp(uri: Uri): String? {
        val displayName = queryDisplayName(uri)
            ?: uri.lastPathSegment?.substringAfterLast('/')
            ?: throw IllegalArgumentException("Could not determine file name")
        val fileName = File(displayName).name
        if (!fileName.endsWith(".mrp", ignoreCase = true)) {
            throw IllegalArgumentException("Select an MRP file")
        }

        val sourceFile = resolveSourceFile(uri)
        if (sourceFile != null && isWithinMythroad(sourceFile)) {
            return null
        }

        if (!mythroadDirectory.isDirectory && !mythroadDirectory.mkdirs()) {
            throw IllegalStateException("Could not create mythroad directory")
        }

        val temporaryFile = File.createTempFile("mrp-import-", ".tmp", cacheDir)
        try {
            val input = contentResolver.openInputStream(uri)
                ?: throw IllegalStateException("Could not open selected file")
            input.use { source ->
                temporaryFile.outputStream().use(source::copyTo)
            }
            temporaryFile.copyTo(
                target = File(mythroadDirectory, fileName),
                overwrite = true,
            )
        } finally {
            temporaryFile.delete()
        }
        return fileName
    }

    private fun queryDisplayName(uri: Uri): String? {
        return contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            if (cursor.moveToFirst()) {
                cursor.getString(0)
            } else {
                null
            }
        }
    }

    private fun resolveSourceFile(uri: Uri): File? {
        if (uri.scheme == "file") {
            return uri.path?.let(::File)
        }
        if (!DocumentsContract.isDocumentUri(this, uri)) {
            return null
        }

        val documentId = DocumentsContract.getDocumentId(uri)
        if (documentId.startsWith("raw:")) {
            return File(documentId.removePrefix("raw:"))
        }
        if (uri.authority != "com.android.externalstorage.documents") {
            return null
        }

        val separator = documentId.indexOf(':')
        if (separator < 0 || !documentId.substring(0, separator).equals(
                "primary",
                ignoreCase = true,
            )
        ) {
            return null
        }
        return File(
            Environment.getExternalStorageDirectory(),
            documentId.substring(separator + 1),
        )
    }

    private fun isWithinMythroad(file: File): Boolean {
        val root = mythroadDirectory.canonicalFile
        val candidate = file.canonicalFile
        return candidate == root ||
            candidate.path.startsWith(root.path + File.separator)
    }

    override fun onResume() {
        super.onResume()
        storageRefreshToken++
    }
}

@Composable
internal fun SkyMRPTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colors = SkyMRPColors,
        content = content,
    )
}

@Composable
private fun LauncherScreen(
    items: List<MrpListItem>,
    listResetToken: Int,
    title: String,
    query: String,
    onQueryChange: (String) -> Unit,
    scanMode: MrpScanMode,
    onScanModeChange: (MrpScanMode) -> Unit,
    canNavigateUp: Boolean,
    onNavigateUp: () -> Unit,
    versionName: String,
    onImport: () -> Unit,
    onItemClick: (MrpListItem) -> Unit,
    onRename: (MrpListItem, String) -> Unit,
    onDelete: (MrpListItem) -> Unit,
    onAddShortcut: (MrpListItem) -> Unit,
    onExit: () -> Unit,
) {
    val context = LocalContext.current
    val keyboardController = LocalSoftwareKeyboardController.current
    val uriHandler = LocalUriHandler.current
    val searchFocusRequester = remember { FocusRequester() }
    val listState = rememberLazyListState()
    val currentYear = remember {
        Calendar.getInstance().get(Calendar.YEAR).coerceAtLeast(COPYRIGHT_START_YEAR)
    }
    val copyrightYears = if (currentYear == COPYRIGHT_START_YEAR) {
        "$COPYRIGHT_START_YEAR"
    } else {
        "$COPYRIGHT_START_YEAR-$currentYear"
    }
    var searchActive by remember { mutableStateOf(false) }
    var moreMenuExpanded by remember { mutableStateOf(false) }
    var aboutVisible by remember { mutableStateOf(false) }
    var detailsItem by remember { mutableStateOf<MrpListItem?>(null) }
    var renameItem by remember { mutableStateOf<MrpListItem?>(null) }
    var renameName by remember { mutableStateOf("") }
    var deleteItem by remember { mutableStateOf<MrpListItem?>(null) }

    LaunchedEffect(searchActive) {
        if (searchActive) {
            searchFocusRequester.requestFocus()
            keyboardController?.show()
        }
    }

    LaunchedEffect(listResetToken) {
        listState.scrollToItem(0)
    }

    Scaffold(
        modifier = Modifier.navigationBarsPadding(),
        floatingActionButton = {
            FloatingActionButton(
                onClick = onImport,
                backgroundColor = MaterialTheme.colors.primary,
                contentColor = MaterialTheme.colors.onPrimary,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Add,
                    contentDescription = "Import MRP",
                )
            }
        },
        topBar = {
            Column(
                modifier = Modifier
                    .background(MaterialTheme.colors.primarySurface)
                    .statusBarsPadding(),
            ) {
                TopAppBar(
                    navigationIcon = if (canNavigateUp) {
                        {
                            IconButton(onClick = onNavigateUp) {
                                Icon(
                                    imageVector = Icons.AutoMirrored.Outlined.ArrowBack,
                                    contentDescription = "Navigate up",
                                )
                            }
                        }
                    } else {
                        null
                    },
                    title = {
                        if (searchActive) {
                            TextField(
                                value = query,
                                onValueChange = onQueryChange,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .focusRequester(searchFocusRequester),
                                placeholder = { Text("Search...") },
                                singleLine = true,
                                colors = TextFieldDefaults.textFieldColors(
                                    backgroundColor = Color.Transparent,
                                    focusedIndicatorColor = Color.Transparent,
                                    unfocusedIndicatorColor = Color.Transparent,
                                ),
                            )
                        } else {
                            Text(
                                text = title,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    },
                    actions = {
                        IconToggleButton(
                            checked = scanMode == MrpScanMode.FOLDER,
                            onCheckedChange = { checked ->
                                onScanModeChange(
                                    if (checked) {
                                        MrpScanMode.FOLDER
                                    } else {
                                        MrpScanMode.PARSED
                                    },
                                )
                            },
                        ) {
                            Icon(
                                painter = painterResource(R.drawable.ic_folder_outlined),
                                contentDescription = if (scanMode == MrpScanMode.FOLDER) {
                                    "Show all MRP files"
                                } else {
                                    "Browse folders"
                                },
                                tint = if (scanMode == MrpScanMode.FOLDER) {
                                    MaterialTheme.colors.primary
                                } else {
                                    LocalContentColor.current.copy(
                                        alpha = LocalContentAlpha.current,
                                    )
                                },
                            )
                        }
                        IconButton(
                            onClick = {
                                searchActive = !searchActive
                                if (!searchActive) {
                                    onQueryChange("")
                                    keyboardController?.hide()
                                }
                            },
                        ) {
                            Icon(
                                imageVector = if (searchActive) {
                                    Icons.Outlined.Close
                                } else {
                                    Icons.Outlined.Search
                                },
                                contentDescription = if (searchActive) {
                                    "Close search"
                                } else {
                                    "Search"
                                },
                            )
                        }
                        Column {
                            IconButton(onClick = { moreMenuExpanded = true }) {
                                Icon(
                                    imageVector = Icons.Outlined.MoreVert,
                                    contentDescription = "More options",
                                )
                            }
                            DropdownMenu(
                                expanded = moreMenuExpanded,
                                onDismissRequest = { moreMenuExpanded = false },
                            ) {
                                DropdownMenuItem(
                                    contentPadding = PaddingValues(
                                        start = 8.dp,
                                        end = 24.dp,
                                    ),
                                    onClick = {
                                        moreMenuExpanded = false
                                        searchActive = false
                                        onQueryChange("")
                                        keyboardController?.hide()
                                        context.startActivity(
                                            Intent(context, SettingsActivity::class.java),
                                        )
                                    },
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.Settings,
                                        contentDescription = null,
                                    )
                                    Spacer(Modifier.width(16.dp))
                                    Text("Settings")
                                }
                                DropdownMenuItem(
                                    contentPadding = PaddingValues(
                                        start = 8.dp,
                                        end = 24.dp,
                                    ),
                                    onClick = {
                                        moreMenuExpanded = false
                                        aboutVisible = true
                                    },
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.Info,
                                        contentDescription = null,
                                    )
                                    Spacer(Modifier.width(16.dp))
                                    Text("About")
                                }
                                DropdownMenuItem(
                                    contentPadding = PaddingValues(
                                        start = 8.dp,
                                        end = 24.dp,
                                    ),
                                    onClick = {
                                        moreMenuExpanded = false
                                        onExit()
                                    },
                                ) {
                                    Icon(
                                        imageVector =
                                            Icons.AutoMirrored.Outlined.ExitToApp,
                                        contentDescription = null,
                                    )
                                    Spacer(Modifier.width(16.dp))
                                    Text("Exit")
                                }
                            }
                        }
                    },
                    elevation = 0.dp,
                )
            }
        },
    ) { contentPadding ->
        if (items.isEmpty()) {
            androidx.compose.foundation.layout.Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(contentPadding),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = if (query.isBlank()) {
                        "No folders or MRP files"
                    } else {
                        "No matching folders or MRP files"
                    },
                    color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                )
            }
        } else {
            LazyColumn(
                state = listState,
                modifier = Modifier
                    .fillMaxSize()
                    .padding(contentPadding),
            ) {
                items(
                    items = items,
                    key = { it.path },
                ) { item ->
                    MrpListRow(
                        item = item,
                        scanMode = scanMode,
                        onClick = { onItemClick(item) },
                        onViewDetails = { detailsItem = item },
                        onRename = {
                            renameItem = item
                            renameName = File(item.path).name
                        },
                        onDelete = { deleteItem = item },
                        onAddShortcut = { onAddShortcut(item) },
                    )
                    Divider(
                        modifier = Modifier.padding(start = 72.dp),
                        color = MaterialTheme.colors.onBackground.copy(alpha = 0.12f),
                    )
                }
            }
        }
    }

    if (aboutVisible) {
        AlertDialog(
            onDismissRequest = { aboutVisible = false },
            title = {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Image(
                        painter = painterResource(R.drawable.icon),
                        contentDescription = null,
                        modifier = Modifier
                            .size(40.dp)
                            .clip(RoundedCornerShape(8.dp)),
                    )
                    Spacer(Modifier.width(12.dp))
                    Text("SkyMRP")
                }
            },
            text = {
                Column {
                    Text("Version: $versionName")
                    Spacer(Modifier.height(4.dp))
                    Row {
                        Text("GitHub: ")
                        Text(
                            text = "open-mythroad/skymrp-emu",
                            modifier = Modifier.clickable {
                                uriHandler.openUri(
                                    "https://github.com/open-mythroad/skymrp-emu",
                                )
                            },
                            color = MaterialTheme.colors.primary,
                        )
                    }
                    Spacer(Modifier.height(8.dp))
                    Text("Copyright $copyrightYears arctan95")
                }
            },
            buttons = {},
        )
    }

    detailsItem?.let { item ->
        AlertDialog(
            onDismissRequest = { detailsItem = null },
            title = { Text("MRP details") },
            text = {
                Column {
                    DetailField("Name", item.name)
                    DetailField("File name", File(item.path).name)
                    DetailField("Vendor", item.vendor.ifBlank { "Unknown" })
                    DetailField(
                        "Description",
                        item.description.ifBlank { "Unknown" },
                    )
                    DetailField(
                        "Version",
                        item.version.takeIf { it > 0 }?.toString() ?: "Unknown",
                    )
                    DetailField("Path", item.path)
                }
            },
            confirmButton = {
                TextButton(onClick = { detailsItem = null }) {
                    Text("Close")
                }
            },
        )
    }

    renameItem?.let { item ->
        AlertDialog(
            onDismissRequest = { renameItem = null },
            title = { Text("Rename MRP") },
            text = {
                TextField(
                    value = renameName,
                    onValueChange = { renameName = it },
                    label = { Text("File name") },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = renameName.isNotBlank() &&
                        renameName.endsWith(".mrp", ignoreCase = true),
                    onClick = {
                        onRename(item, renameName.trim())
                        renameItem = null
                    },
                ) {
                    Text("Rename")
                }
            },
            dismissButton = {
                TextButton(onClick = { renameItem = null }) {
                    Text("Cancel")
                }
            },
        )
    }

    deleteItem?.let { item ->
        AlertDialog(
            onDismissRequest = { deleteItem = null },
            title = { Text("Delete MRP") },
            text = { Text("Delete ${item.name} permanently?") },
            confirmButton = {
                TextButton(
                    onClick = {
                        onDelete(item)
                        deleteItem = null
                    },
                ) {
                    Text("Delete")
                }
            },
            dismissButton = {
                TextButton(onClick = { deleteItem = null }) {
                    Text("Cancel")
                }
            },
        )
    }
}

@Composable
@OptIn(ExperimentalFoundationApi::class)
private fun MrpListRow(
    item: MrpListItem,
    scanMode: MrpScanMode,
    onClick: () -> Unit,
    onViewDetails: () -> Unit,
    onRename: () -> Unit,
    onDelete: () -> Unit,
    onAddShortcut: () -> Unit,
) {
    var actionsExpanded by remember { mutableStateOf(false) }
    var pressPosition by remember { mutableStateOf(Offset.Zero) }
    val interactionModifier = if (item.isDirectory) {
        Modifier.clickable(onClick = onClick)
    } else {
        Modifier
            .pointerInput(item.path) {
                awaitEachGesture {
                    pressPosition = awaitFirstDown(
                        requireUnconsumed = false,
                    ).position
                    do {
                        val event = awaitPointerEvent()
                    } while (event.changes.any { it.pressed })
                }
            }
            .combinedClickable(
                onClick = onClick,
                onLongClick = { actionsExpanded = true },
            )
    }

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .then(interactionModifier),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (item.isDirectory) {
                Icon(
                    painter = painterResource(R.drawable.ic_folder),
                    contentDescription = null,
                    modifier = Modifier.size(40.dp),
                    tint = MaterialTheme.colors.primary,
                )
            } else {
                Image(
                    painter = painterResource(R.drawable.ic_mrp_app),
                    contentDescription = null,
                    modifier = Modifier.size(40.dp),
                )
            }
            Spacer(Modifier.width(16.dp))
            Column(
                modifier = Modifier.weight(1f),
            ) {
                Text(
                    text = item.name,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (item.vendor.isNotBlank()) {
                    Spacer(Modifier.height(3.dp))
                    Text(
                        text = item.vendor,
                        color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.body2,
                    )
                }
                if (item.parentPath.isNotBlank()) {
                    Spacer(Modifier.height(3.dp))
                    Text(
                        text = item.parentPath,
                        color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.body2,
                    )
                }
            }
            if (!item.isDirectory && item.version > 0) {
                Spacer(Modifier.width(12.dp))
                Text(
                    text = item.version.toString(),
                    color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                    style = MaterialTheme.typography.body2,
                )
            }
        }
        if (!item.isDirectory) {
            Box(
                modifier = Modifier
                    .offset {
                        IntOffset(
                            pressPosition.x.roundToInt(),
                            pressPosition.y.roundToInt(),
                        )
                    }
                    .size(1.dp),
            ) {
                DropdownMenu(
                    expanded = actionsExpanded,
                    onDismissRequest = { actionsExpanded = false },
                ) {
                    if (scanMode == MrpScanMode.PARSED) {
                        DropdownMenuItem(
                            onClick = {
                                actionsExpanded = false
                                onViewDetails()
                            },
                        ) {
                            Text("View details")
                        }
                    } else {
                        DropdownMenuItem(
                            onClick = {
                                actionsExpanded = false
                                onRename()
                            },
                        ) {
                            Text("Rename")
                        }
                    }
                    DropdownMenuItem(
                        onClick = {
                            actionsExpanded = false
                            onAddShortcut()
                        },
                    ) {
                        Text("Add shortcut")
                    }
                    DropdownMenuItem(
                        onClick = {
                            actionsExpanded = false
                            onDelete()
                        },
                    ) {
                        Text("Delete")
                    }
                }
            }
        }
    }
}

@Composable
private fun DetailField(label: String, value: String) {
    Text(
        text = label,
        color = MaterialTheme.colors.onSurface.copy(alpha = 0.6f),
        style = MaterialTheme.typography.caption,
    )
    Text(value)
    Spacer(Modifier.height(8.dp))
}

@Preview(
    name = "MRP list",
    showBackground = true,
    backgroundColor = 0xFF101214,
    widthDp = 360,
    heightDp = 720,
)
@Composable
private fun LauncherScreenPreview() {
    SkyMRPTheme {
        LauncherScreen(
            items = listOf(
                MrpListItem(
                    name = "plugins",
                    path = "/mythroad/plugins",
                    vendor = "",
                    version = 0,
                    isDirectory = true,
                ),
                MrpListItem(
                    name = "蜀山剑侠传",
                    path = "/mythroad/gssjxz.mrp",
                    vendor = "杭州斯凯",
                    description = "Classic role-playing game",
                    version = 1006,
                ),
                MrpListItem(
                    name = "新大唐双龙传",
                    path = "/mythroad/gxdtslz.mrp",
                    vendor = "杭州斯凯",
                    version = 1006,
                ),
                MrpListItem(
                    name = "QQ2010",
                    path = "/mythroad/atxqq.mrp",
                    vendor = "腾讯",
                    version = 1022,
                ),
                MrpListItem(
                    name = "手机QQ浏览器",
                    path = "/mythroad/atxllq.mrp",
                    vendor = "杭州斯凯",
                    version = 1040,
                ),
                MrpListItem(
                    name = "冒泡",
                    path = "/mythroad/cookie.mrp",
                    vendor = "sky",
                    version = 5040,
                ),
            ),
            listResetToken = 0,
            title = "SkyMRP",
            query = "",
            onQueryChange = {},
            scanMode = MrpScanMode.FOLDER,
            onScanModeChange = {},
            canNavigateUp = false,
            onNavigateUp = {},
            versionName = "0.3.1",
            onImport = {},
            onItemClick = {},
            onRename = { _, _ -> },
            onDelete = {},
            onAddShortcut = {},
            onExit = {},
        )
    }
}
