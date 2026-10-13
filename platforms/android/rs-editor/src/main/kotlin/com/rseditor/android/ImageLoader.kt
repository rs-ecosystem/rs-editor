package com.rseditor.android

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.net.Uri
import android.util.Base64
import java.net.HttpURLConnection
import java.net.URL
import java.io.ByteArrayInputStream
import java.net.URLDecoder
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Loads the pictures of image nodes off the UI thread: `data:` addresses, content and file URIs
 * through the content resolver, and `http(s)` addresses (the app needs the INTERNET permission).
 * BitmapFactory decodes them, at most [MAX_SIZE] pixels on the longer side, and the editor gets
 * the pixels on the UI thread.
 */
internal class ImageLoader(private val view: RsEditorView) {
    /** A decoded picture: premultiplied RGBA and pixels per CSS pixel. */
    class Decoded(val rgba: ByteArray, val width: Int, val height: Int, val scale: Float)

    private val executor: ExecutorService = Executors.newFixedThreadPool(2)

    fun load(src: String) {
        executor.execute {
            val decoded = try {
                decode(src)
            } catch (_: Exception) {
                null
            } catch (_: OutOfMemoryError) {
                null
            }
            view.post { view.imageLoaded(src, decoded) }
        }
    }

    fun release() {
        executor.shutdownNow()
    }

    private fun decode(src: String): Decoded? {
        val bytes = read(src) ?: return null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / sample > MAX_SIZE) sample *= 2
        val options = BitmapFactory.Options().apply {
            inSampleSize = sample
            inPreferredConfig = Bitmap.Config.ARGB_8888
        }
        val decoded = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options) ?: return null
        // Photos keep their orientation in EXIF; BitmapFactory ignores it.
        val bitmap = oriented(decoded, bytes)
        try {
            // An ARGB_8888 bitmap's memory is premultiplied RGBA.
            val buffer = ByteBuffer.allocate(bitmap.byteCount)
            bitmap.copyPixelsToBuffer(buffer)
            return Decoded(
                buffer.array(),
                bitmap.width,
                bitmap.height,
                // The longer sides: a quarter turn swaps width and height.
                maxOf(bitmap.width, bitmap.height).toFloat() / maxOf(bounds.outWidth, bounds.outHeight),
            )
        } finally {
            bitmap.recycle()
        }
    }

    /** `bitmap` turned and mirrored as the image's EXIF orientation says (a new bitmap if so). */
    private fun oriented(bitmap: Bitmap, bytes: ByteArray): Bitmap {
        val orientation = try {
            ExifInterface(ByteArrayInputStream(bytes))
                .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
        } catch (_: Exception) {
            ExifInterface.ORIENTATION_NORMAL
        }
        val matrix = Matrix()
        when (orientation) {
            ExifInterface.ORIENTATION_ROTATE_90 -> matrix.postRotate(90f)
            ExifInterface.ORIENTATION_ROTATE_180 -> matrix.postRotate(180f)
            ExifInterface.ORIENTATION_ROTATE_270 -> matrix.postRotate(270f)
            ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> matrix.postScale(-1f, 1f)
            ExifInterface.ORIENTATION_FLIP_VERTICAL -> matrix.postScale(1f, -1f)
            ExifInterface.ORIENTATION_TRANSPOSE -> {
                matrix.postRotate(90f)
                matrix.postScale(-1f, 1f)
            }
            ExifInterface.ORIENTATION_TRANSVERSE -> {
                matrix.postRotate(270f)
                matrix.postScale(-1f, 1f)
            }
            else -> return bitmap
        }
        val turned = Bitmap.createBitmap(bitmap, 0, 0, bitmap.width, bitmap.height, matrix, true)
        if (turned !== bitmap) bitmap.recycle()
        return turned
    }

    private fun read(src: String): ByteArray? {
        val scheme = src.substringBefore(':', "").lowercase()
        return when (scheme) {
            "data" -> {
                val meta = src.substringAfter(':').substringBefore(',')
                val data = src.substringAfter(',', "")
                if (meta.endsWith(";base64")) {
                    Base64.decode(data, Base64.DEFAULT)
                } else {
                    URLDecoder.decode(data, "UTF-8").toByteArray()
                }
            }
            "http", "https" -> {
                val connection = URL(src).openConnection() as HttpURLConnection
                connection.connectTimeout = 15_000
                connection.readTimeout = 30_000
                try {
                    connection.inputStream.use { it.readBytes() }
                } finally {
                    connection.disconnect()
                }
            }
            else -> view.context.contentResolver.openInputStream(Uri.parse(src))?.use { it.readBytes() }
        }
    }

    private companion object {
        const val MAX_SIZE = 2048
    }
}
