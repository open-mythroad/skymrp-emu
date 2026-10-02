/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;

public final class StoragePermissionActivity extends Activity {
    private static final int STORAGE_PERMISSION_REQUEST = 1;

    private boolean permissionRequested;
    private boolean mainActivityStarted;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        if (hasStorageAccess()) {
            startMainActivity();
        } else {
            requestStorageAccess();
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (permissionRequested && hasStorageAccess()) {
            startMainActivity();
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == STORAGE_PERMISSION_REQUEST) {
            if (hasStorageAccess()) {
                startMainActivity();
            } else {
                finish();
            }
        }
    }

    @Override
    public void onRequestPermissionsResult(
        int requestCode,
        String[] permissions,
        int[] grantResults
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == STORAGE_PERMISSION_REQUEST) {
            if (hasStorageAccess()) {
                startMainActivity();
            } else {
                finish();
            }
        }
    }

    private boolean hasStorageAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            return true;
        }
        return checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                == PackageManager.PERMISSION_GRANTED
            && checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE)
                == PackageManager.PERMISSION_GRANTED;
    }

    private void requestStorageAccess() {
        permissionRequested = true;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Intent intent = new Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:" + getPackageName())
            );
            try {
                startActivityForResult(intent, STORAGE_PERMISSION_REQUEST);
            } catch (Exception ignored) {
                startActivityForResult(
                    new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION),
                    STORAGE_PERMISSION_REQUEST
                );
            }
            return;
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            requestPermissions(
                new String[]{
                    Manifest.permission.READ_EXTERNAL_STORAGE,
                    Manifest.permission.WRITE_EXTERNAL_STORAGE
                },
                STORAGE_PERMISSION_REQUEST
            );
        }
    }

    private void startMainActivity() {
        if (mainActivityStarted) {
            return;
        }
        mainActivityStarted = true;
        startActivity(new Intent(this, MainActivity.class));
        finish();
    }
}
