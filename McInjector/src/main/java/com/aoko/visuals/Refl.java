package com.aoko.visuals;

import java.lang.reflect.Field;
import java.lang.reflect.Method;

/** Reflection lookups used by both platforms. Failures return null instead of throwing. */
final class Refl {
    private Refl() {}

    static Class<?> cls(ClassLoader loader, String name) {
        try {
            return Class.forName(name, false, loader);
        } catch (Throwable t) {
            return null;
        }
    }

    /** First declared field with one of the names, searching the class and its superclasses. */
    static Field field(Class<?> c, String[] names) {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            for (String n : names) {
                try {
                    Field f = k.getDeclaredField(n);
                    f.setAccessible(true);
                    return f;
                } catch (NoSuchFieldException e) {
                    // try the next name
                } catch (Throwable t) {
                    return null;
                }
            }
        }
        return null;
    }

    static Field field(Class<?> c, String name) {
        return field(c, new String[] { name });
    }

    /** First declared method with one of the names and exactly these parameters, searching superclasses. */
    static Method method(Class<?> c, String[] names, Class<?>... params) {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            for (String n : names) {
                try {
                    Method m = k.getDeclaredMethod(n, params);
                    m.setAccessible(true);
                    return m;
                } catch (NoSuchMethodException e) {
                    // try the next name
                } catch (Throwable t) {
                    return null;
                }
            }
        }
        return null;
    }

    static Method method(Class<?> c, String name, Class<?>... params) {
        return method(c, new String[] { name }, params);
    }

    /** A public method found through the whole type hierarchy, including interfaces. */
    static Method publicMethod(Class<?> c, String name, Class<?>... params) {
        try {
            Method m = c.getMethod(name, params);
            m.setAccessible(true);
            return m;
        } catch (Throwable t) {
            return null;
        }
    }
}
