package com.aoko.visuals;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Small strict JSON reader. Objects become {@code Map<String,Object>}, arrays
 * {@code List<Object>}, numbers {@code Double}, plus String, Boolean and null.
 * Bounded in depth and size so a hostile or corrupt asset cannot hurt the game.
 */
final class Json {
    static final int MAX_CHARS = 1 << 20;
    private static final int MAX_DEPTH = 40;

    static final class ParseException extends Exception {
        private static final long serialVersionUID = 1L;
        ParseException(String message) { super(message); }
    }

    private final String s;
    private int i;

    private Json(String s) { this.s = s; }

    static Object parse(String text) throws ParseException {
        if (text == null) throw new ParseException("null input");
        if (text.length() > MAX_CHARS) throw new ParseException("input too large");
        Json p = new Json(text);
        p.skipWs();
        if (p.i < text.length() && text.charAt(p.i) == '\uFEFF') { p.i++; p.skipWs(); }
        Object v = p.value(0);
        p.skipWs();
        if (p.i != text.length()) throw new ParseException("trailing characters at " + p.i);
        return v;
    }

    private void skipWs() {
        while (i < s.length()) {
            char c = s.charAt(i);
            if (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\uFEFF') i++; else break;
        }
    }

    private Object value(int depth) throws ParseException {
        if (depth > MAX_DEPTH) throw new ParseException("nesting too deep");
        skipWs();
        if (i >= s.length()) throw new ParseException("unexpected end");
        char c = s.charAt(i);
        switch (c) {
            case '{': return object(depth);
            case '[': return array(depth);
            case '"': return string();
            case 't': expect("true"); return Boolean.TRUE;
            case 'f': expect("false"); return Boolean.FALSE;
            case 'n': expect("null"); return null;
            default:
                if (c == '-' || (c >= '0' && c <= '9')) return number();
                throw new ParseException("unexpected '" + c + "' at " + i);
        }
    }

    private void expect(String word) throws ParseException {
        if (!s.startsWith(word, i)) throw new ParseException("bad literal at " + i);
        i += word.length();
    }

    private Map<String, Object> object(int depth) throws ParseException {
        Map<String, Object> map = new LinkedHashMap<String, Object>();
        i++;
        skipWs();
        if (i < s.length() && s.charAt(i) == '}') { i++; return map; }
        while (true) {
            skipWs();
            if (i >= s.length() || s.charAt(i) != '"') throw new ParseException("expected key at " + i);
            String key = string();
            skipWs();
            if (i >= s.length() || s.charAt(i) != ':') throw new ParseException("expected ':' at " + i);
            i++;
            map.put(key, value(depth + 1));
            skipWs();
            if (i >= s.length()) throw new ParseException("unterminated object");
            char c = s.charAt(i++);
            if (c == '}') return map;
            if (c != ',') throw new ParseException("expected ',' at " + (i - 1));
        }
    }

    private List<Object> array(int depth) throws ParseException {
        List<Object> list = new ArrayList<Object>();
        i++;
        skipWs();
        if (i < s.length() && s.charAt(i) == ']') { i++; return list; }
        while (true) {
            list.add(value(depth + 1));
            skipWs();
            if (i >= s.length()) throw new ParseException("unterminated array");
            char c = s.charAt(i++);
            if (c == ']') return list;
            if (c != ',') throw new ParseException("expected ',' at " + (i - 1));
        }
    }

    private String string() throws ParseException {
        i++; // opening quote
        StringBuilder sb = new StringBuilder();
        while (i < s.length()) {
            char c = s.charAt(i++);
            if (c == '"') return sb.toString();
            if (c == '\\') {
                if (i >= s.length()) break;
                char e = s.charAt(i++);
                switch (e) {
                    case '"': sb.append('"'); break;
                    case '\\': sb.append('\\'); break;
                    case '/': sb.append('/'); break;
                    case 'b': sb.append('\b'); break;
                    case 'f': sb.append('\f'); break;
                    case 'n': sb.append('\n'); break;
                    case 'r': sb.append('\r'); break;
                    case 't': sb.append('\t'); break;
                    case 'u':
                        if (i + 4 > s.length()) throw new ParseException("bad unicode escape");
                        try { sb.append((char) Integer.parseInt(s.substring(i, i + 4), 16)); }
                        catch (NumberFormatException ex) { throw new ParseException("bad unicode escape"); }
                        i += 4;
                        break;
                    default: throw new ParseException("bad escape \\" + e);
                }
            } else if (c < 0x20) {
                throw new ParseException("control character in string");
            } else {
                sb.append(c);
            }
        }
        throw new ParseException("unterminated string");
    }

    private Double number() throws ParseException {
        int start = i;
        if (s.charAt(i) == '-') i++;
        while (i < s.length()) {
            char c = s.charAt(i);
            if ((c >= '0' && c <= '9') || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-') i++; else break;
        }
        try {
            double d = Double.parseDouble(s.substring(start, i));
            if (Double.isNaN(d) || Double.isInfinite(d)) throw new ParseException("non-finite number");
            return Double.valueOf(d);
        } catch (NumberFormatException ex) {
            throw new ParseException("bad number at " + start);
        }
    }

    // ---- Typed accessors (lenient: wrong type or missing gives the default) ----

    @SuppressWarnings("unchecked")
    static Map<String, Object> obj(Object o) { return o instanceof Map ? (Map<String, Object>) o : null; }

    @SuppressWarnings("unchecked")
    static List<Object> arr(Object o) { return o instanceof List ? (List<Object>) o : null; }

    static String str(Map<String, Object> m, String key, String def) {
        if (m == null) return def;
        Object v = m.get(key);
        return v instanceof String ? (String) v : def;
    }

    static double num(Map<String, Object> m, String key, double def) {
        if (m == null) return def;
        Object v = m.get(key);
        return v instanceof Double ? ((Double) v).doubleValue() : def;
    }

    static boolean bool(Map<String, Object> m, String key, boolean def) {
        if (m == null) return def;
        Object v = m.get(key);
        return v instanceof Boolean ? ((Boolean) v).booleanValue() : def;
    }

    /** Reads a numeric array of exactly {@code n} elements, or returns {@code def}. */
    static double[] nums(Object o, int n, double[] def) {
        List<Object> l = arr(o);
        if (l == null || l.size() != n) return def;
        double[] out = new double[n];
        for (int k = 0; k < n; k++) {
            Object e = l.get(k);
            if (!(e instanceof Double)) return def;
            out[k] = ((Double) e).doubleValue();
        }
        return out;
    }

    /** Escapes a string as a JSON string literal (with quotes). */
    static String quote(String v) {
        StringBuilder sb = new StringBuilder(v.length() + 2);
        sb.append('"');
        for (int k = 0; k < v.length(); k++) {
            char c = v.charAt(k);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c)); else sb.append(c);
            }
        }
        return sb.append('"').toString();
    }
}
