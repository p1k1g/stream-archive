using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.System;
using Windows.UI;
using Windows.UI.ViewManagement;

namespace SOOPLiveWinUI;

internal static class DesignTokens
{
    static readonly UISettings SystemUi = new();

    internal static readonly bool HighContrast = GetHighContrast();
    internal static readonly SolidColorBrush AppBackground = Semantic(0x25, 0x2A, 0x31, UIColorType.Background);
    internal static readonly SolidColorBrush Surface = Semantic(0x20, 0x25, 0x2C, UIColorType.Background);
    internal static readonly SolidColorBrush SurfaceElevated = Semantic(0x2A, 0x30, 0x38, UIColorType.Background);
    internal static readonly SolidColorBrush Border = Semantic(0x3A, 0x41, 0x4A, UIColorType.Foreground);
    internal static readonly SolidColorBrush TextPrimary = Semantic(0xFF, 0xFF, 0xFF, UIColorType.Foreground);
    internal static readonly SolidColorBrush TextSecondary = Semantic(0xA7, 0xB0, 0xBE, UIColorType.Foreground);
    internal static readonly SolidColorBrush Accent = Semantic(0x42, 0xD9, 0x87, UIColorType.Accent);
    internal static readonly SolidColorBrush AccentContent = Semantic(0x10, 0x21, 0x18, UIColorType.Background);
    internal static readonly SolidColorBrush Warning = Semantic(0xF4, 0xC9, 0x5D, UIColorType.Accent);
    internal static readonly SolidColorBrush Danger = Semantic(0xF0, 0x80, 0x80, UIColorType.Accent);

    internal const double SpaceXs = 4;
    internal const double SpaceSm = 8;
    internal const double SpaceMd = 12;
    internal const double SpaceLg = 16;
    internal const double SpaceXl = 22;
    internal const double ControlHeight = 36;
    internal const double CompactChannelWidth = 900;
    internal const double CompactRecentWidth = 1050;

    internal static readonly CornerRadius CardRadius = new(10);
    internal static readonly CornerRadius BadgeRadius = new(999);
    internal static readonly Thickness CardPadding = new(SpaceMd);
    internal static readonly Thickness PagePadding = new(SpaceXl);

    internal static Button StyleButton(Button button, double minWidth = 92, bool primary = false)
    {
        button.MinHeight = ControlHeight;
        button.MinWidth = minWidth;
        button.Padding = new Thickness(14, 6, 14, 6);
        button.CornerRadius = new CornerRadius(6);
        if (primary)
        {
            button.Background = Accent;
            button.Foreground = AccentContent;
        }
        return button;
    }

    internal static AppBarButton Command(string label, Symbol symbol, bool enabled = true)
    {
        var button = new AppBarButton
        {
            Label = label,
            Icon = new SymbolIcon(symbol),
            IsEnabled = enabled
        };
        AutomationProperties.SetName(button, label);
        ToolTipService.SetToolTip(button, label);
        return button;
    }

    internal static void AddAccelerator(UIElement owner, VirtualKey key, VirtualKeyModifiers modifiers)
    {
        owner.KeyboardAccelerators.Add(new KeyboardAccelerator { Key = key, Modifiers = modifiers });
    }

    internal static void AddAccelerator(
        UIElement owner,
        VirtualKey key,
        VirtualKeyModifiers modifiers,
        Action invoked)
    {
        var accelerator = new KeyboardAccelerator { Key = key, Modifiers = modifiers };
        accelerator.Invoked += (_, args) =>
        {
            invoked();
            args.Handled = true;
        };
        owner.KeyboardAccelerators.Add(accelerator);
    }

    internal static SolidColorBrush StateBrush(byte red, byte green, byte blue, UIColorType fallback) =>
        Semantic(red, green, blue, fallback);

    static SolidColorBrush Semantic(byte red, byte green, byte blue, UIColorType highContrastColor)
    {
        var color = HighContrast
            ? SystemUi.GetColorValue(highContrastColor)
            : Color.FromArgb(255, red, green, blue);
        return new SolidColorBrush(color);
    }

    static bool GetHighContrast()
    {
        try { return new AccessibilitySettings().HighContrast; }
        catch { return false; }
    }
}
