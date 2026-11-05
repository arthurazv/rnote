#import <AppKit/AppKit.h>
#import <QuickLookUI/QuickLookUI.h>
#import "RnoteDocRenderer.h"

@interface RnotePreviewViewController : NSViewController <QLPreviewingController>
@property (nonatomic, strong) NSImageView *imageView;
@property (nonatomic, strong) NSScrollView *scrollView;
@end

@implementation RnotePreviewViewController

- (void)loadView {
    NSScrollView *scrollView = [[NSScrollView alloc] initWithFrame:NSMakeRect(0, 0, 900, 1100)];
    scrollView.hasVerticalScroller = YES;
    scrollView.hasHorizontalScroller = YES;
    scrollView.autohidesScrollers = YES;
    scrollView.drawsBackground = NO;

    NSImageView *imageView = [[NSImageView alloc] initWithFrame:NSMakeRect(0, 0, 900, 1100)];
    imageView.imageScaling = NSImageScaleProportionallyUpOrDown;
    imageView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;

    scrollView.documentView = imageView;
    self.imageView = imageView;
    self.scrollView = scrollView;
    self.view = scrollView;
}

- (void)preparePreviewOfFileAtURL:(NSURL *)url completionHandler:(void (^)(NSError * _Nullable))handler {
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSImage *image = RnoteRenderDocument(url.path, CGSizeMake(1200, 1600));
        dispatch_async(dispatch_get_main_queue(), ^{
            if (image) {
                self.imageView.image = image;
                self.preferredContentSize = image.size;
                handler(nil);
            } else {
                handler([NSError errorWithDomain:@"com.github.flxzt.rnote.quicklook" code:1 userInfo:nil]);
            }
        });
    });
}

@end
