#import <AppKit/AppKit.h>
#import <QuickLookThumbnailing/QuickLookThumbnailing.h>
#import "RnoteDocRenderer.h"

@interface RnoteThumbnailProvider : QLThumbnailProvider
@end

@implementation RnoteThumbnailProvider

- (void)provideThumbnailForFileRequest:(QLFileThumbnailRequest *)request completionHandler:(void (^)(QLThumbnailReply * _Nullable reply, NSError * _Nullable error))handler {
    NSImage *image = RnoteRenderDocument(request.fileURL.path, request.maximumSize);
    if (image) {
        QLThumbnailReply *reply = [QLThumbnailReply replyWithContextSize:request.maximumSize currentContextDrawingBlock:^BOOL{
            [image drawInRect:NSMakeRect(0, 0, request.maximumSize.width, request.maximumSize.height)
                     fromRect:NSZeroRect
                    operation:NSCompositingOperationCopy
                     fraction:1.0];
            return YES;
        }];
        handler(reply, nil);
    } else {
        handler(nil, nil);
    }
}

@end
