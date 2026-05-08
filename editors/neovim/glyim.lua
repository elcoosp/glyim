local configs = require('lspconfig.configs')

if not configs.glyim then
  configs.glyim = {
    default_config = {
      cmd = { 'glyim', 'lsp' },
      filetypes = { 'glyim' },
      root_dir = function(fname)
        return require('lspconfig.util').root_pattern('glyim.toml')(fname)
      end,
      settings = {},
    },
  }
end

-- Example usage:
-- require('lspconfig').glyim.setup{}
