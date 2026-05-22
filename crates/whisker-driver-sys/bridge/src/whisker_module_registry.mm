#import "whisker_module_registry.h"

// Backing storage: two parallel dicts, one for the class
// registration (set at app launch, read on every invoke) and one
// for the lazy-singleton instances (read-mostly after the first
// invoke per module). Both guarded by a single `NSLock` — module
// dispatch isn't on the hot path, so coarse locking is fine.

@implementation WhiskerModuleRegistry

+ (NSMutableDictionary<NSString *, Class> *)classesMap {
    static NSMutableDictionary<NSString *, Class> *m;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      m = [NSMutableDictionary new];
    });
    return m;
}

+ (NSMutableDictionary<NSString *, id> *)instancesMap {
    static NSMutableDictionary<NSString *, id> *m;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      m = [NSMutableDictionary new];
    });
    return m;
}

+ (NSLock *)mapLock {
    static NSLock *l;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      l = [NSLock new];
    });
    return l;
}

+ (void)registerModuleClass:(Class)cls forName:(NSString *)name {
    if (cls == nil || name == nil) return;
    NSLock *lock = [self mapLock];
    [lock lock];
    [self classesMap][name] = cls;
    // Drop any cached instance — the new class supersedes the
    // previous lazy-singleton (rare in practice; mostly relevant
    // when hot-patches reload a module class).
    [[self instancesMap] removeObjectForKey:name];
    [lock unlock];
}

+ (Class)moduleClassForName:(NSString *)name {
    if (name == nil) return nil;
    NSLock *lock = [self mapLock];
    [lock lock];
    Class cls = [self classesMap][name];
    [lock unlock];
    return cls;
}

+ (id)moduleInstanceForName:(NSString *)name {
    if (name == nil) return nil;
    NSLock *lock = [self mapLock];
    [lock lock];
    id instance = [self instancesMap][name];
    if (instance == nil) {
        Class cls = [self classesMap][name];
        if (cls != nil) {
            instance = [[cls alloc] init];
            if (instance != nil) {
                [self instancesMap][name] = instance;
            }
        }
    }
    [lock unlock];
    return instance;
}

@end
