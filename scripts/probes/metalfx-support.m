// SPDX-License-Identifier: MIT
// Original capability probe. No window, game, account or user-data access.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <MetalFX/MetalFX.h>
#include <stdio.h>
int main(void) {
    @autoreleasepool {
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        BOOL supported = device && [MTLFXSpatialScalerDescriptor supportsDevice:device];
        printf("{\"schema\":1,\"metalfx_spatial_supported\":%s,\"game_performance\":\"not_tested\"}\n", supported ? "true" : "false");
        return supported ? 0 : 1;
    }
}
