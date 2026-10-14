/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
var Module = typeof Module !== "undefined" ? Module : {};

(function () {
    var resolveRuntime;
    var runtimeReady = new Promise(function (resolve) {
        resolveRuntime = resolve;
    });
    var scriptUrl =
        typeof document !== "undefined" && document.currentScript
            ? document.currentScript.src
            : null;
    var assetBaseUrl = scriptUrl ? new URL(".", scriptUrl) : null;
    var fontsReady;
    var previousOnRuntimeInitialized = Module["onRuntimeInitialized"];
    var started = false;

    Module["onRuntimeInitialized"] = function () {
        if (previousOnRuntimeInitialized) {
            previousOnRuntimeInitialized();
        }
        resolveRuntime();
    };

    Module["loadMrp"] = async function (source, filename) {
        if (started) {
            throw new Error("SkyMRP has already started");
        }

        var data;
        if (typeof source === "string") {
            if (typeof require !== "function") {
                throw new TypeError("File paths are only supported in Node.js");
            }
            var nodePath = require("path");
            data = new Uint8Array(require("fs").readFileSync(source));
            filename = filename || nodePath.basename(source);
        } else if (source instanceof ArrayBuffer) {
            data = new Uint8Array(source);
        } else if (ArrayBuffer.isView(source)) {
            data = new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
        } else if (source && typeof source.arrayBuffer === "function") {
            filename = filename || source.name;
            data = new Uint8Array(await source.arrayBuffer());
        } else {
            throw new TypeError("Expected a file path, File, ArrayBuffer, or typed array");
        }

        filename = (filename || "cookie.mrp").split(/[\\/]/).pop();
        var mrpPath = "/mythroad/" + filename;

        await runtimeReady;
        await loadDefaultFonts();
        Module["FS"].mkdirTree("/mythroad");
        Module["FS"].writeFile(mrpPath, data);
        started = true;
        Module["callMain"]([mrpPath]);
    };

    function loadDefaultFonts() {
        if (!assetBaseUrl || typeof fetch !== "function") {
            return Promise.resolve();
        }
        if (fontsReady) {
            return fontsReady;
        }

        fontsReady = Promise.all(
            ["gb12.uc2", "gb16.uc2"].map(async function (filename) {
                var response = await fetch(new URL("fonts/" + filename, assetBaseUrl));
                if (!response.ok) {
                    throw new Error(
                        "Could not load " + filename + ": HTTP " + response.status
                    );
                }
                return [filename, new Uint8Array(await response.arrayBuffer())];
            })
        ).then(function (fonts) {
            Module["FS"].mkdirTree("/mythroad/system");
            for (var font of fonts) {
                Module["FS"].writeFile("/mythroad/system/" + font[0], font[1]);
            }
        });
        return fontsReady;
    }
})();
