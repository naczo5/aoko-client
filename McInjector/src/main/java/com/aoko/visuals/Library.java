package com.aoko.visuals;

import java.awt.image.BufferedImage;
import java.io.ByteArrayInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;
import javax.imageio.ImageIO;
import javax.imageio.ImageReader;
import javax.imageio.stream.ImageInputStream;
import javax.imageio.stream.MemoryCacheImageInputStream;

/**
 * Reads assets from the visuals directory. Every file read is bounded, every id is
 * validated, and parsed results are cached until the file changes on disk.
 */
final class Library {
    static final int MAX_JSON_BYTES = 512 * 1024;
    static final int MAX_IMAGE_BYTES = 6 * 1024 * 1024;
    static final int MAX_IMAGE_SIDE = 4096;

    private static final class Cached<T> {
        final T value;
        final long stamp;
        Cached(T value, long stamp) { this.value = value; this.stamp = stamp; }
    }

    final File root;
    private final Map<String, Cached<Cosmetic>> cosmetics = new HashMap<String, Cached<Cosmetic>>();
    private final Map<String, Cached<SkyPreset>> skies = new HashMap<String, Cached<SkyPreset>>();
    private final Map<String, Cached<BufferedImage>> images = new HashMap<String, Cached<BufferedImage>>();

    Library(File root) { this.root = root; }

    File capeFile(String id) { return Config.ID.matcher(id).matches() ? new File(root, "capes/" + id + ".png") : null; }
    File cosmeticDir(String id) { return Config.ID.matcher(id).matches() ? new File(root, "cosmetics/" + id) : null; }
    File skyDir(String id) { return Config.ID.matcher(id).matches() ? new File(root, "skies/" + id) : null; }

    private static long stampOf(File... files) {
        long s = 17;
        for (File f : files) s = s * 31 + f.lastModified() * 7 + f.length();
        return s;
    }

    synchronized Cosmetic loadCosmetic(String id) throws IOException, Json.ParseException {
        File dir = cosmeticDir(id);
        if (dir == null) throw new IOException("bad cosmetic id");
        File manifest = new File(dir, "cosmetic.json");
        // The model file name comes from the manifest, so read it first to build the stamp.
        String manifestText = readText(manifest, MAX_JSON_BYTES);
        Map<String, Object> m = Json.obj(Json.parse(manifestText));
        String modelName = Json.str(m, "model", "model.geo.json");
        if (!Cosmetic.safeFileName(modelName)) throw new Json.ParseException("bad model file name");
        File model = new File(dir, modelName);
        long stamp = stampOf(manifest, model);
        Cached<Cosmetic> c = cosmetics.get(id);
        if (c != null && c.stamp == stamp) return c.value;
        Cosmetic parsed = Cosmetic.parse(id, manifestText, readText(model, MAX_JSON_BYTES));
        cosmetics.put(id, new Cached<Cosmetic>(parsed, stamp));
        return parsed;
    }

    synchronized SkyPreset loadSky(String id) throws IOException, Json.ParseException {
        File dir = skyDir(id);
        if (dir == null) throw new IOException("bad sky id");
        File f = new File(dir, "sky.json");
        long stamp = stampOf(f);
        Cached<SkyPreset> c = skies.get(id);
        if (c != null && c.stamp == stamp) return c.value;
        SkyPreset parsed = SkyPreset.parse(id, readText(f, MAX_JSON_BYTES));
        skies.put(id, new Cached<SkyPreset>(parsed, stamp));
        return parsed;
    }

    /** Stamp that changes when the file changes; used by callers that cache GPU copies. */
    long stamp(File f) { return stampOf(f); }

    synchronized BufferedImage loadImage(File f) throws IOException {
        long stamp = stampOf(f);
        String key = f.getPath();
        Cached<BufferedImage> c = images.get(key);
        if (c != null && c.stamp == stamp) return c.value;
        BufferedImage img = readImage(f);
        if (images.size() > 24) images.clear();
        images.put(key, new Cached<BufferedImage>(img, stamp));
        return img;
    }

    static String readText(File f, int max) throws IOException {
        return new String(readBytes(f, max), "UTF-8");
    }

    static byte[] readBytes(File f, int max) throws IOException {
        if (!f.isFile()) throw new IOException("missing file " + f.getName());
        long len = f.length();
        if (len > max) throw new IOException(f.getName() + " is larger than " + max + " bytes");
        byte[] out = new byte[(int) len];
        InputStream in = new FileInputStream(f);
        try {
            int off = 0;
            while (off < out.length) {
                int n = in.read(out, off, out.length - off);
                if (n < 0) throw new IOException("short read " + f.getName());
                off += n;
            }
        } finally {
            in.close();
        }
        return out;
    }

    /** Decodes PNG or JPEG after checking the dimensions, so a huge image is never allocated. */
    static BufferedImage readImage(File f) throws IOException {
        byte[] data = readBytes(f, MAX_IMAGE_BYTES);
        // Memory backed on purpose: ImageIO.createImageInputStream(InputStream) spools to a temp file.
        ImageInputStream in = new MemoryCacheImageInputStream(new ByteArrayInputStream(data));
        try {
            Iterator<ImageReader> readers = ImageIO.getImageReaders(in);
            if (!readers.hasNext()) throw new IOException("unknown image format " + f.getName());
            ImageReader r = readers.next();
            try {
                r.setInput(in, true, true);
                int w = r.getWidth(0), h = r.getHeight(0);
                if (w < 1 || h < 1 || w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE)
                    throw new IOException(f.getName() + " has unsupported size " + w + "x" + h);
                BufferedImage img = r.read(0);
                if (img == null) throw new IOException("unreadable image " + f.getName());
                return img;
            } finally {
                r.dispose();
            }
        } finally {
            in.close();
        }
    }

    /** Sky atlas layout check: 3 x 2 grid of equal square faces. */
    static boolean validAtlas(BufferedImage img) {
        int w = img.getWidth(), h = img.getHeight();
        return w % 3 == 0 && h % 2 == 0 && w / 3 == h / 2 && w / 3 >= 16;
    }

    /** Vanilla cape layout: 2:1, width a multiple of 64. */
    static boolean validCape(BufferedImage img) {
        int w = img.getWidth(), h = img.getHeight();
        return w >= 64 && w % 64 == 0 && h * 2 == w;
    }
}
