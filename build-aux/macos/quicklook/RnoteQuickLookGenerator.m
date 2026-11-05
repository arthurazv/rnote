#import <Foundation/Foundation.h>
#import <CoreFoundation/CoreFoundation.h>
#import <CoreFoundation/CFPlugInCOM.h>
#import <QuickLook/QuickLook.h>
#import <unistd.h>

static NSString *findRnoteCli() {
    NSArray *candidates = @[
        @"/Applications/Rnote.app/Contents/MacOS/rnote-cli",
        @"/tmp/rnote_install/bin/rnote-cli",
        @"/usr/local/bin/rnote-cli",
        @"/opt/homebrew/bin/rnote-cli"
    ];
    for (NSString *p in candidates) {
        if ([[NSFileManager defaultManager] isExecutableFileAtPath:p]) {
            return p;
        }
    }
    return nil;
}

static NSData *renderRnoteToPng(NSString *rnotePath, int size) {
    NSString *cli = findRnoteCli();
    if (!cli) return nil;

    char tempTemplate[] = "/tmp/rnote_qlg_XXXXXX.png";
    int fd = mkstemps(tempTemplate, 4);
    if (fd < 0) return nil;
    close(fd);
    NSString *tempPath = [NSString stringWithUTF8String:tempTemplate];

    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:cli];
    task.arguments = @[
        @"thumbnail",
        @"-s", [NSString stringWithFormat:@"%d", size],
        @"-t", @"8",
        rnotePath,
        tempPath
    ];
    task.standardOutput = [NSFileHandle fileHandleWithNullDevice];
    task.standardError = [NSFileHandle fileHandleWithNullDevice];

    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        unlink(tempTemplate);
        return nil;
    }
    [task waitUntilExit];

    NSData *data = nil;
    if (task.terminationStatus == 0) {
        data = [NSData dataWithContentsOfFile:tempPath];
    }
    unlink(tempTemplate);
    return data;
}

static HRESULT QueryInterface(void *thisInstance, REFIID iid, LPVOID *ppv) {
    CFUUIDRef interfaceID = CFUUIDCreateFromUUIDBytes(kCFAllocatorDefault, iid);
    if (CFEqual(interfaceID, kQLGeneratorCallbacksInterfaceID) ||
        CFEqual(interfaceID, IUnknownUUID)) {
        *ppv = thisInstance;
        CFRelease(interfaceID);
        return S_OK;
    }
    CFRelease(interfaceID);
    *ppv = NULL;
    return E_NOINTERFACE;
}

static ULONG AddRef(void *thisInstance) { return 1; }
static ULONG Release(void *thisInstance) { return 1; }

static OSStatus MyGenerateThumbnailForURL(void *thisInterface, QLThumbnailRequestRef thumbnail, CFURLRef url, CFStringRef contentTypeUTI, CFDictionaryRef options, CGSize maxSize) {
    @autoreleasepool {
        if (QLThumbnailRequestIsCancelled(thumbnail)) return noErr;
        NSString *filePath = [(__bridge NSURL *)url path];
        int size = (int)MAX(maxSize.width, maxSize.height);
        if (size <= 0) size = 512;
        if (size > 1024) size = 1024;

        NSData *pngData = renderRnoteToPng(filePath, size);
        if (pngData && !QLThumbnailRequestIsCancelled(thumbnail)) {
            QLThumbnailRequestSetImageWithData(thumbnail, (__bridge CFDataRef)pngData, NULL);
        }
    }
    return noErr;
}

static void MyCancelThumbnailGeneration(void *thisInterface, QLThumbnailRequestRef thumbnail) {}

static OSStatus MyGeneratePreviewForURL(void *thisInterface, QLPreviewRequestRef preview, CFURLRef url, CFStringRef contentTypeUTI, CFDictionaryRef options) {
    @autoreleasepool {
        if (QLPreviewRequestIsCancelled(preview)) return noErr;
        NSString *filePath = [(__bridge NSURL *)url path];
        NSData *pngData = renderRnoteToPng(filePath, 1600);
        if (pngData && !QLPreviewRequestIsCancelled(preview)) {
            QLPreviewRequestSetDataRepresentation(preview, (__bridge CFDataRef)pngData, CFSTR("public.png"), NULL);
        }
    }
    return noErr;
}

static void MyCancelPreviewGeneration(void *thisInterface, QLPreviewRequestRef preview) {}

static QLGeneratorInterfaceStruct gInterface = {
    NULL,
    QueryInterface,
    AddRef,
    Release,
    MyGenerateThumbnailForURL,
    MyCancelThumbnailGeneration,
    MyGeneratePreviewForURL,
    MyCancelPreviewGeneration
};

static void *gInstance = &gInterface;

void *QuickLookGeneratorPluginFactory(CFAllocatorRef allocator, CFUUIDRef typeID) {
    if (CFEqual(typeID, kQLGeneratorTypeID)) {
        return &gInstance;
    }
    return NULL;
}
