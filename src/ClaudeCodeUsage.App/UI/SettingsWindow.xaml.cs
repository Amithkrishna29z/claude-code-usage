using System.Globalization;
using System.Windows;
using ClaudeCodeUsage.Core;
using ClaudeCodeUsage.Core.Models;
using WinForms = System.Windows.Forms;
using MessageBox = System.Windows.MessageBox;
using MessageBoxButton = System.Windows.MessageBoxButton;
using MessageBoxImage = System.Windows.MessageBoxImage;

namespace ClaudeCodeUsage.App.UI;

public partial class SettingsWindow : Window
{
    private readonly AppConfig _config;

    public SettingsWindow(AppConfig config, string configPath)
    {
        InitializeComponent();
        _config = config;

        OfficialCheck.IsChecked = config.UseOfficialUsage;
        LimitBox.Text = config.TokenLimit.ToString(CultureInfo.InvariantCulture);
        HoursBox.Text = config.WindowHours.ToString(CultureInfo.InvariantCulture);
        DirBox.Text = config.ClaudeDir;
        StartupCheck.IsChecked = config.StartWithWindows;
        ConfigPathText.Text = $"Config file: {configPath}";
    }

    private void OnBrowse(object sender, RoutedEventArgs e)
    {
        using var dialog = new WinForms.FolderBrowserDialog
        {
            Description = "Select your .claude directory",
            UseDescriptionForTitle = true,
        };
        var start = UsageReader.ResolveClaudeDir(DirBox.Text);
        if (System.IO.Directory.Exists(start)) dialog.SelectedPath = start;

        if (dialog.ShowDialog() == WinForms.DialogResult.OK)
            DirBox.Text = dialog.SelectedPath;
    }

    private void OnSave(object sender, RoutedEventArgs e)
    {
        if (!long.TryParse(LimitBox.Text.Trim().Replace(",", ""),
                NumberStyles.Integer, CultureInfo.InvariantCulture, out var limit) || limit <= 0)
        {
            Warn("Token limit must be a positive whole number.");
            return;
        }

        if (!double.TryParse(HoursBox.Text.Trim(), NumberStyles.Float,
                CultureInfo.InvariantCulture, out var hours) || hours <= 0)
        {
            Warn("Window length must be a positive number of hours.");
            return;
        }

        _config.UseOfficialUsage = OfficialCheck.IsChecked == true;
        _config.TokenLimit = limit;
        _config.WindowHours = hours;
        _config.ClaudeDir = DirBox.Text.Trim();
        _config.StartWithWindows = StartupCheck.IsChecked == true;

        DialogResult = true;
        Close();
    }

    private void OnCancel(object sender, RoutedEventArgs e)
    {
        DialogResult = false;
        Close();
    }

    private void Warn(string message) =>
        MessageBox.Show(this, message, "Invalid setting",
            MessageBoxButton.OK, MessageBoxImage.Warning);
}
